#![no_std]

pub mod types;
pub mod bus;
mod enumeration;
mod transfer;

use core::num::NonZeroU8;
use bus::HostBus;
use types::{DeviceAddress, DescriptorType, SetupPacket};
use enumeration::EnumerationState;
use usb_device::{UsbDirection, control::{Recipient, RequestType, Request}};
use defmt::debug;

#[derive(Copy, Clone)]
enum State {
    Enumeration(EnumerationState),
    Assigned(DeviceAddress),
}

pub struct WouldBlock;

pub enum Event {
    None,
    Attached(types::ConnectionSpeed),
    Detached,
    DelayComplete,
    ControlInData,
    ControlOutComplete,
    Stall,
    Resume,
    BusError(bus::Error),
}

/// Result returned from `UsbHost::poll`.
pub enum PollResult {
    /// Nothing special to report. Poll again when another interrupt happens
    None,
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
        self.bus.set_address(dev_addr, 0);
        self.bus.write_setup(setup);

        Ok(())
    }

    pub fn control_out(&mut self, dev_addr: Option<DeviceAddress>, setup: SetupPacket, data: &[u8]) -> Result<(), WouldBlock> {
        if self.current_transfer.is_some() {
            return Err(WouldBlock)
        }

        self.current_transfer = Some(transfer::Transfer::new_control_out(data.len() as u16));
        self.bus.set_address(dev_addr, 0);
        self.bus.prepare_data_out(data);
        self.bus.write_setup(setup);

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

    /// Poll the USB host. This must be called reasonably often.
    ///
    /// If the host implementation has an interrupt that fires on USB activity, then calling it once in that interrupt handler is enough.
    /// Otherwise make sure to call it at least once per millisecond.
    ///
    /// By default `delay_complete` should be passed as `false`.
    /// Only if `PollResult::PollAgain` was returned, `poll(true)` should be called once after the delay has passed.
    pub fn poll(&mut self, delay_complete: bool) -> PollResult {
        let bus_result = self.bus.poll();

        let mut poll_result = PollResult::None;

        let event = if delay_complete {
            debug!("USB delay complete");
            Event::DelayComplete
        } else if let Some(event) = bus_result.event {
            debug!("USB host event {}", event);
            match event {
                bus::Event::Attached(speed) => Event::Attached(speed),
                bus::Event::Detached => Event::Detached,
                bus::Event::WriteComplete => {
                    if let Some(transfer) = self.current_transfer.take() {
                        match transfer.stage_complete(self) {
                            transfer::PollResult::ControlInComplete(length) => Event::ControlInData,
                            transfer::PollResult::ControlOutComplete => Event::ControlOutComplete,
                            transfer::PollResult::Continue(transfer) => {
                                self.current_transfer = Some(transfer);
                                Event::None
                            }
                        }
                    } else {
                        panic!("BUG: received WriteComplete while no transfer was in progress")
                    }
                },
                bus::Event::Resume => Event::Resume,
                bus::Event::Stall => {
                    // TODO: figure out if we should reset everything in case of a stall, or just ignore it until the device is unplugged
                    Event::Stall
                },
                bus::Event::Error(error) => Event::BusError(error),
            }
        } else {
            Event::None
        };

        match &self.state {
            State::Enumeration(enumeration_state) => {
                match enumeration::process_enumeration(event, *enumeration_state, self) {
                    EnumerationState::Assigned(address) => {
                        self.state = State::Assigned(address);
                    }
                    state if state.delay().is_some() => {
                        self.state = State::Enumeration(state);
                        poll_result = PollResult::PollAgain(state.delay().unwrap())
                    },
                    other => {
                        self.state = State::Enumeration(other);
                    }
                }
            }

            State::Assigned(device_address) => {
                self.bus.enable_sof();
                // TODO: fetch all the descriptors
            }
        }

        poll_result
    }
}
