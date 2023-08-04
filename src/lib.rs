#![no_std]

pub mod types;
pub mod bus;
pub mod driver;

//pub mod drivers;
//pub mod pipe;

mod transfer;
mod enumeration;
mod discovery;

pub mod descriptor;

use core::num::NonZeroU8;
use bus::HostBus;
use types::{DeviceAddress, DescriptorType, SetupPacket, TransferType};
use enumeration::EnumerationState;
use discovery::DiscoveryState;
use usb_device::{UsbDirection, control::{Recipient, RequestType, Request}};
use defmt::{info, debug, Format};

const MAX_PIPES: usize = 32;

#[derive(Copy, Clone)]
enum State {
    Enumeration(EnumerationState),
    Discovery(DeviceAddress, DiscoveryState),
    Configuring(DeviceAddress, u8),
    Configured(DeviceAddress, u8),
    // No driver is interested.
    Dormant(DeviceAddress),
}

#[derive(Debug)]
pub struct WouldBlock;

#[derive(Copy, Clone, Format)]
pub enum Event {
    None,
    Attached(types::ConnectionSpeed),
    Detached,
    ControlInData(Option<PipeId>, u16),
    ControlOutComplete(Option<PipeId>),
    Stall,
    Resume,
    InterruptPipe(u8),
    BusError(bus::Error),
    Sof,
}

/// Result returned from `UsbHost::poll`.
pub enum PollResult {
    /// There is no device attached. It does not make sense to do anything else with the UsbHost instance, until a device was attached.
    NoDevice,
    /// Bus is currently busy talking to a device. Calling any transfer methods on the device will return a `WouldBlock` error.
    Busy,
    /// A device is attached and the bus is available. The caller can use the UsbHost instance to start a transfer or configure an interrupt.
    Idle,

    BusError(bus::Error),
}

pub struct UsbHost<B> {
    bus: B,
    state: State,
    active_transfer: Option<(Option<PipeId>, transfer::Transfer)>,
    last_address: u8,
    pipes: [Option<Pipe>; MAX_PIPES],
}

unsafe impl<B> Send for UsbHost<B> {}

#[derive(Copy, Clone)]
enum Pipe {
    Control {
        dev_addr: DeviceAddress,
    },
    Interrupt {
        dev_addr: DeviceAddress,
        bus_ref: u8,
        direction: UsbDirection,
        size: u16,
        ptr: *mut u8,
    }
}

#[derive(Copy, Clone, PartialEq, Format)]
pub struct PipeId(u8);

impl<B: HostBus> UsbHost<B> {
    pub fn new(mut bus: B) -> Self {
        bus.reset_controller();
        Self {
            bus,
            state: State::Enumeration(EnumerationState::WaitForDevice),
            active_transfer: None,
            last_address: 0,
            pipes: [None; MAX_PIPES],
        }
    }

    fn alloc_pipe(&mut self) -> Option<(PipeId, &mut Option<Pipe>)> {
        self.pipes.iter_mut().enumerate().find(|(_, slot)| slot.is_none()).map(|(i, slot)| (PipeId(i as u8), slot))
    }

    pub fn create_control_pipe(&mut self, dev_addr: DeviceAddress) -> Option<PipeId> {
        self.alloc_pipe().map(|(id, slot)| {
            slot.replace(Pipe::Control { dev_addr });
            id
        })
    }

    pub fn create_interrupt_pipe(&mut self, dev_addr: DeviceAddress, ep_number: u8, direction: UsbDirection, size: u16, interval: u8) -> Option<PipeId> {
        self.bus().create_interrupt_pipe(dev_addr, ep_number, direction, size, interval)
            .and_then(|(ptr, bus_ref)| {
                self.alloc_pipe().map(|(id, slot)| {
                    slot.replace(Pipe::Interrupt { dev_addr, bus_ref, direction, size, ptr });
                    id
                })
            })
    }

    pub fn reset(&mut self) {
        self.bus.reset_controller();
        self.state = State::Enumeration(EnumerationState::WaitForDevice);
        self.active_transfer = None;
        self.last_address = 0;
        self.pipes = [None; MAX_PIPES];
    }

    /// Returns the next unassigned address, and increments the counter
    fn next_address(&mut self) -> DeviceAddress {
        self.last_address = self.last_address.wrapping_add(1);
        if self.last_address == 0 {
            self.last_address += 1;
        }
        DeviceAddress(NonZeroU8::new(self.last_address).unwrap())
    }

    fn control_in(&mut self, dev_addr: Option<DeviceAddress>, pipe_id: Option<PipeId>, setup: SetupPacket, length: u16) -> Result<(), WouldBlock> {
        if self.active_transfer.is_some() {
            return Err(WouldBlock)
        }

        self.active_transfer = Some((pipe_id, transfer::Transfer::new_control_in(length)));
        self.bus.set_recipient(dev_addr, 0, TransferType::Control);
        self.bus.write_setup(setup);

        Ok(())
    }

    fn control_out(&mut self, dev_addr: Option<DeviceAddress>, pipe_id: Option<PipeId>, setup: SetupPacket, data: &[u8]) -> Result<(), WouldBlock> {
        if self.active_transfer.is_some() {
            return Err(WouldBlock)
        }

        self.active_transfer = Some((pipe_id, transfer::Transfer::new_control_out(data.len() as u16)));
        self.bus.set_recipient(dev_addr, 0, TransferType::Control);
        self.bus.prepare_data_out(data);
        self.bus.write_setup(setup);

        Ok(())
    }

    pub fn get_descriptor(&mut self, dev_addr: Option<DeviceAddress>, recipient: Recipient, descriptor_type: DescriptorType, descriptor_index: u8, length: u16) -> Result<(), WouldBlock> {
        defmt::info!("GetDescriptor {} {} {} {} {}", dev_addr, recipient, descriptor_type, descriptor_index, length);
        self.control_in(dev_addr, None, SetupPacket::new(
            UsbDirection::In,
            RequestType::Standard,
            recipient,
            Request::GET_DESCRIPTOR,
            ((descriptor_type as u16) << 8) | (descriptor_index as u16),
            0,
            length,
        ), length)
    }

    pub fn set_address(&mut self, address: DeviceAddress) -> Result<(), WouldBlock> {
        self.control_out(None, None, SetupPacket::new(
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
        self.control_out(Some(address), None, SetupPacket::new(
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
    pub fn poll(&mut self, driver: &mut dyn driver::Driver<B>) -> PollResult {
        let bus_result = self.bus.poll();

        let event = if let Some(event) = bus_result.event {
            if event != bus::Event::Sof {
                //debug!("[UsbHost] Bus Event {}", event);
            }
            match event {
                bus::Event::Attached(speed) => Event::Attached(speed),
                bus::Event::Detached => Event::Detached,
                bus::Event::TransComplete => {
                    if let Some((pipe_id, transfer)) = self.active_transfer.take() {
                        match transfer.stage_complete(self) {
                            transfer::PollResult::ControlInComplete(length) => Event::ControlInData(pipe_id, length),
                            transfer::PollResult::ControlOutComplete => Event::ControlOutComplete(pipe_id),
                            transfer::PollResult::Continue(transfer) => {
                                self.active_transfer = Some((pipe_id, transfer));
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
                    Event::BusError(error)
                },
                bus::Event::InterruptPipe(buf_ref) => {
                    Event::InterruptPipe(buf_ref)
                }
                bus::Event::Sof => Event::Sof,
            }
        } else {
            info!("??");
            Event::None
        };

        match &self.state {
            State::Enumeration(enumeration_state) => {
                match enumeration::process_enumeration(event, *enumeration_state, self) {
                    EnumerationState::Assigned(speed, dev_addr) => {
                        info!("[UsbHost] Assigned address {} (speed: {})", dev_addr, speed);
                        driver.attached(dev_addr, speed);
                        let discovery_state = discovery::start_discovery(dev_addr, self);
                        self.state = State::Discovery(dev_addr, discovery_state);
                    }
                    other => {
                        self.state = State::Enumeration(other);
                    }
                };
            }

            State::Discovery(dev_addr, discovery_state) => {
                let dev_addr = *dev_addr;
                match discovery::process_discovery(event, dev_addr, *discovery_state, driver, self) {
                    DiscoveryState::Done => {
                        if let Some(config) = driver.configure(dev_addr) {
                            self.set_configuration(dev_addr, config);
                            self.state = State::Configuring(dev_addr, config);
                        } else {
                            self.state = State::Dormant(dev_addr);
                        }
                    }
                    other => {
                        self.state = State::Discovery(dev_addr, other);
                    }
                }
            }

            State::Configuring(dev_addr, config) => {
                let dev_addr = *dev_addr;
                let config = *config;
                match event {
                    Event::ControlOutComplete(_) => {
                        driver.configured(dev_addr, config, self);
                        self.state = State::Configured(dev_addr, config);
                    }
                    Event::Detached => {
                        driver.detached(dev_addr);
                        self.reset();
                    }
                    _ => {}
                }
            }

            State::Configured(dev_addr, _config) => {
                match event {
                    Event::Detached => {
                        driver.detached(*dev_addr);
                        self.reset();
                    }

                    Event::ControlInData(pipe_id, len) => {
                        if let Some(pipe_id) = pipe_id {
                            let data = unsafe { self.bus.control_buffer(len as usize) };
                            driver.completed_control(*dev_addr, pipe_id, Some(data));
                        }
                    },

                    Event::ControlOutComplete(pipe_id) => {
                        if let Some(pipe_id) = pipe_id {
                            driver.completed_control(*dev_addr, pipe_id, None);
                        }
                    },

                    Event::InterruptPipe(pipe_ref) => {
                        let matching_pipe = self.pipes.iter()
                            .enumerate()
                            .find(|(_, pipe)| {
                                if let Some(Pipe::Interrupt { bus_ref, .. }) = pipe {
                                    *bus_ref == pipe_ref
                                } else {
                                    false
                                }
                            })
                            .map(|(id, pipe)| (PipeId(id as u8), pipe.unwrap()));

                        if let Some((pipe_id, Pipe::Interrupt { dev_addr, size, ptr, direction, .. })) = matching_pipe {
                            match direction {
                                UsbDirection::In => {
                                    let buf = unsafe { core::slice::from_raw_parts(ptr, size as usize) };
                                    driver.completed_in(dev_addr, pipe_id, buf);
                                },
                                UsbDirection::Out => {
                                    let buf = unsafe { core::slice::from_raw_parts_mut(ptr, size as usize) };
                                    driver.completed_out(dev_addr, pipe_id, buf);
                                },
                            }
                        }
                        self.bus.pipe_continue(pipe_ref);
                    },

                    Event::BusError(error) => {
                        return PollResult::BusError(error)
                    }

                    _ => {}
                }
            }

            State::Dormant(dev_addr) => {
                match event {
                    Event::Detached => {
                        driver.detached(*dev_addr);
                        self.reset();
                    }
                    _ => {}
                }
            }
        }

        if let State::Enumeration(EnumerationState::WaitForDevice) = self.state {
            PollResult::NoDevice
        } else if self.active_transfer.is_some() {
            PollResult::Busy
        } else {
            PollResult::Idle
        }
    }
}
