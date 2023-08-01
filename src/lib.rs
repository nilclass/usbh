#![no_std]

pub mod types;
pub mod bus;
pub mod driver;
mod enumeration;
mod transfer;

use core::num::NonZeroU8;
use bus::HostBus;
use types::{DeviceAddress, DescriptorType, SetupPacket, TransferType};
use enumeration::EnumerationState;
use usb_device::{UsbDirection, control::{Recipient, RequestType, Request}};
use defmt::{info, debug, Format};

#[derive(Copy, Clone)]
enum State {
    Enumeration(EnumerationState),
    Assigned(DeviceAddress),
}

pub struct WouldBlock;

#[derive(Copy, Clone, Format)]
pub enum Event {
    None,
    Attached(types::ConnectionSpeed),
    Detached,
    DelayComplete,
    ControlInData(u16),
    ControlOutComplete,
    InterruptInComplete(u16),
    InterruptOutComplete,
    Stall,
    Resume,
    BusError(bus::Error),
}

/// Result returned from `UsbHost::poll`.
pub enum PollResult {
    /// There is no device attached. It does not make sense to do anything else with the UsbHost instance, until a device was attached.
    NoDevice,
    /// Bus is currently busy talking to a device. Calling any transfer methods on the device will return a `WouldBlock` error.
    Busy,
    /// A device is attached and the bus is available. The caller can use the UsbHost instance to start a transfer or configure an interrupt.
    Idle,
    /// Poll again after the given duration. This is used to implement delays in the enumeration process, without blocking.
    PollAgain(fugit::MillisDurationU32),
}

pub struct UsbHost<B> {
    bus: B,
    state: State,
    current_transfer: Option<transfer::Transfer>,
    last_address: u8,
}

impl<B: HostBus> UsbHost<B> {
    pub fn new(mut bus: B) -> Self {
        bus.reset_controller();
        Self {
            bus,
            state: State::Enumeration(EnumerationState::WaitForDevice),
            current_transfer: None,
            last_address: 0,
        }
    }

    pub fn reset(&mut self) {
        self.bus.reset_controller();
        self.state = State::Enumeration(EnumerationState::WaitForDevice);
        self.current_transfer = None;
        self.last_address = 0;
    }

    /// Returns the next unassigned address, and increments the counter
    fn next_address(&mut self) -> DeviceAddress {
        self.last_address = self.last_address.wrapping_add(1);
        if self.last_address == 0 {
            self.last_address += 1;
        }
        DeviceAddress(NonZeroU8::new(self.last_address).unwrap())
    }

    pub fn control_in(&mut self, dev_addr: Option<DeviceAddress>, setup: SetupPacket, length: u16) -> Result<(), WouldBlock> {
        if self.current_transfer.is_some() {
            return Err(WouldBlock)
        }

        self.current_transfer = Some(transfer::Transfer::new_control_in(length));
        self.bus.set_recipient(dev_addr, 0, TransferType::Control);
        self.bus.write_setup(setup);

        Ok(())
    }

    pub fn control_out(&mut self, dev_addr: Option<DeviceAddress>, setup: SetupPacket, data: &[u8]) -> Result<(), WouldBlock> {
        if self.current_transfer.is_some() {
            return Err(WouldBlock)
        }

        self.current_transfer = Some(transfer::Transfer::new_control_out(data.len() as u16));
        self.bus.set_recipient(dev_addr, 0, TransferType::Control);
        self.bus.prepare_data_out(data);
        self.bus.write_setup(setup);

        Ok(())
    }

    pub fn interrupt_in(&mut self, dev_addr: DeviceAddress, ep: u8, length: u16) -> Result<(), WouldBlock> {
        if self.current_transfer.is_some() {
            return Err(WouldBlock)
        }

        self.current_transfer = Some(transfer::Transfer::new_interrupt_in(length));
        self.bus.set_recipient(Some(dev_addr), ep, TransferType::Interrupt);
        self.bus.write_data_in(length);

        Ok(())
    }

    pub fn get_descriptor(&mut self, dev_addr: Option<DeviceAddress>, recipient: Recipient, descriptor_type: DescriptorType, length: u16) -> Result<(), WouldBlock> {
        self.control_in(dev_addr, SetupPacket::new(
            UsbDirection::In,
            RequestType::Standard,
            recipient,
            Request::GET_DESCRIPTOR,
            (descriptor_type as u16) << 8,
            0,
            length,
        ), length)
    }

    pub fn set_address(&mut self, address: DeviceAddress) -> Result<(), WouldBlock> {
        self.control_out(None, SetupPacket::new(
            UsbDirection::Out,
            RequestType::Standard,
            Recipient::Device,
            Request::SET_ADDRESS,
            address.into(),
            0,
            0,
        ), &[])
    }

    pub fn set_configuration(&mut self, address: DeviceAddress, configuration: u8) -> Result<(), WouldBlock> {
        debug!("[UsbHost:{}] SetConfiguration({})", address, configuration);
        self.control_out(Some(address), SetupPacket::new(
            UsbDirection::Out,
            RequestType::Standard,
            Recipient::Device,
            Request::SET_CONFIGURATION,
            configuration as u16,
            0,
            0,
        ), &[])
    }

    pub fn bus(&mut self) -> &mut B {
        &mut self.bus
    }

    /// Poll the USB host. This must be called reasonably often.
    ///
    /// If the host implementation has an interrupt that fires on USB activity, then calling it once in that interrupt handler is enough.
    /// Otherwise make sure to call it at least once per millisecond.
    ///
    /// By default `delay_complete` should be passed as `false`.
    /// Only if `PollResult::PollAgain` was returned, `poll(true)` should be called once after the delay has passed.
    pub fn poll(&mut self, delay_complete: bool, driver: &mut dyn driver::Driver<B>) -> PollResult {
        let bus_result = self.bus.poll();

        let event = if delay_complete {
            Event::DelayComplete
        } else if let Some(event) = bus_result.event {
            debug!("[UsbHost] Bus Event {}", event);
            match event {
                bus::Event::Attached(speed) => Event::Attached(speed),
                bus::Event::Detached => Event::Detached,
                bus::Event::WriteComplete => {
                    if let Some(transfer) = self.current_transfer.take() {
                        match transfer.stage_complete(self) {
                            transfer::PollResult::ControlInComplete(length) => Event::ControlInData(length),
                            transfer::PollResult::ControlOutComplete => Event::ControlOutComplete,
                            transfer::PollResult::InterruptInComplete(length) => Event::InterruptInComplete(length),
                            transfer::PollResult::InterruptOutComplete => Event::InterruptOutComplete,
                            transfer::PollResult::Continue(transfer) => {
                                self.current_transfer = Some(transfer);
                                Event::None
                            }
                        }
                    } else {
                        panic!("BUG: received WriteComplete while no transfer was in progress")
                    }
                },
                bus::Event::Resume => {
                    info!("[UsbHost] Device resumed");
                    Event::Resume
                },
                bus::Event::Stall => {
                    info!("[UsbHost] Stall received!");
                    // TODO: figure out if we should reset everything in case of a stall, or just ignore it until the device is unplugged
                    Event::Stall
                },
                bus::Event::Error(error) => {
                    panic!("ERROR");
                    Event::BusError(error)
                },
                bus::Event::InterruptData(x) => {
                    info!("INTERRUPT DATA AVAILABLE??? buff status {}", x);
                    Event::None
                }
            }
        } else {
            info!("??");
            Event::None
        };

        let mut delay = None;

        match &self.state {
            State::Enumeration(enumeration_state) => {
                match enumeration::process_enumeration(event, *enumeration_state, self) {
                    EnumerationState::Assigned(address) => {
                        info!("[UsbHost] Assigned address {}", address);
                        self.state = State::Assigned(address);
                        driver.attached(address, self);
                    }
                    state if state.delay().is_some() => {
                        self.state = State::Enumeration(state);
                        delay = state.delay();
                    },
                    other => {
                        self.state = State::Enumeration(other);
                    }
                };
            }

            State::Assigned(device_address) => {
                match event {
                    Event::Detached => {
                        driver.detached(*device_address);
                        self.bus.reset_controller();
                    }
                    Event::ControlInData(len) => driver.transfer_in_complete(*device_address, len as usize, self),
                    Event::ControlOutComplete => driver.transfer_out_complete(*device_address, self),
                    Event::InterruptInComplete(len) => driver.interrupt_in_complete(*device_address, len as usize, self),
                    Event::InterruptOutComplete => driver.interrupt_out_complete(*device_address, self),
                    _ => {}
                }
            }
        }

        if let Some(delay) = delay {
            PollResult::PollAgain(delay)
        } else if let State::Enumeration(EnumerationState::WaitForDevice) = self.state {
            PollResult::NoDevice
        } else if self.current_transfer.is_some() {
            PollResult::Busy
        } else {
            PollResult::Idle
        }
    }
}
