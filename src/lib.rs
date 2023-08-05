//!


#![no_std]


pub mod types;
pub mod bus;
pub mod driver;

mod transfer;
mod enumeration;
mod discovery;

pub mod descriptor;

use core::num::NonZeroU8;
use bus::HostBus;
use types::{DeviceAddress, SetupPacket, TransferType};
use enumeration::EnumerationState;
use discovery::DiscoveryState;
use usb_device::{UsbDirection, control::{Recipient, RequestType, Request}};
use defmt::{info, Format};

/// Maximum number of pipes that the host supports.
const MAX_PIPES: usize = 32;

#[derive(Copy, Clone)]
enum State {
    // Enumeration phase: starts in WaitForDevice state, ends with an address being assigned
    Enumeration(EnumerationState),
    // Discovery phase: starts with an assigned address, ends with a configuration being chosen
    Discovery(DeviceAddress, DiscoveryState),
    // Configuration phase: put the device into the chosen configuration
    Configuring(DeviceAddress, u8),
    // 
    Configured(DeviceAddress, u8),
    // No driver is interested, or the device misbehaved during one of the previous phases
    Dormant(DeviceAddress),
}

#[derive(Copy, Clone, PartialEq)]
pub enum ControlError {
    WouldBlock,
    InvalidPipe,
}

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
    /// Bus is currently busy talking to a device. Calling any transfer methods on the device will return a [`ControlError::WouldBlock`] error.
    Busy,
    /// A device is attached and the bus is available. The caller can use the UsbHost instance to start a transfer or configure an interrupt.
    Idle,

    BusError(bus::Error),

    DiscoveryError(DeviceAddress),
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

    /// Create a pipe for control transfers
    ///
    /// This method is meant to be called by drivers.
    ///
    /// The returned `PipeId` can be used to initiate transfers by calling [`UsbHost::control_out`], [`UsbHost::control_in`] or one of their wrappers.
    ///
    /// Returns `None` if the maximum number of supported pipes has been reached.
    pub fn create_control_pipe(&mut self, dev_addr: DeviceAddress) -> Option<PipeId> {
        self.alloc_pipe().map(|(id, slot)| {
            slot.replace(Pipe::Control { dev_addr });
            id
        })
    }

    /// Create a pipe for interrupt transfers
    ///
    /// This method is meant to be called by drivers.
    ///
    /// Transfers on the interrupt pipe are always initiated by the host controller at the appropriate times.
    ///
    /// Drivers must implement the [`driver::Driver::completed_in`] / [`driver::Driver::completed_out`] callbacks to
    /// consume / produce data for the pipe as needed. The returned `PipeId` will be passed to those callbacks for the
    /// driver to be able to associate the calls with an individual pipe they created.
    ///
    /// Returns `None` if the maximum number of supported pipes has been reached.
    pub fn create_interrupt_pipe(&mut self, dev_addr: DeviceAddress, ep_number: u8, direction: UsbDirection, size: u16, interval: u8) -> Option<PipeId> {
        self.bus().create_interrupt_pipe(dev_addr, ep_number, direction, size, interval)
            .and_then(|(ptr, bus_ref)| {
                self.alloc_pipe().map(|(id, slot)| {
                    slot.replace(Pipe::Interrupt { dev_addr, bus_ref, direction, size, ptr });
                    id
                })
            })
    }

    /// Reset the entire host stack
    ///
    /// This resets the host controller (via [`bus::HostBus::reset_controller`]) and resets
    /// all internal state of the UsbHost to their defaults.
    ///
    /// Any current transfer will never complete, and any pipes created will no longer be valid.
    /// At the end of the reset, no device will be connected.
    ///
    /// Drivers must never call this method.
    ///
    /// NOTE: since the host does not keep track of any drivers, it cannot reset the drivers' internal state.
    ///   It is up to application code to reset / re-initialize the drivers after resetting the host stack.
    ///   Any `PipeId` or `DeviceAddress` held by the application or driver(s) must be considered invalid after a reset.
    ///   Continuing to use them can lead to strange behavior, since after a reset, pipe and device addresses *will* be re-used.
    pub fn reset(&mut self) {
        self.bus.reset_controller();
        self.state = State::Enumeration(EnumerationState::WaitForDevice);
        self.active_transfer = None;
        self.last_address = 0;
        self.pipes = [None; MAX_PIPES];
    }

    /// Returns the next unassigned address, and increments the counter
    ///
    /// If 
    fn next_address(&mut self) -> DeviceAddress {
        self.last_address = self.last_address.wrapping_add(1);
        if self.last_address == 0 {
            self.last_address += 1;
        }
        DeviceAddress(NonZeroU8::new(self.last_address).unwrap())
    }

    /// Initiate an IN transfer on the control endpoint of the given device
    ///
    /// If a `pipe_id` is given, the driver that set up the pipe will be able to associate the [`driver::Driver::completed_control`]
    /// call with this transfer.
    /// Otherwise the transfer will not be reported to any drivers.
    ///
    /// The number of bytes transferred is determined by the `length` from the setup packet.
    ///
    /// If there is currently a transfer in progress, [`ControlError::WouldBlock`] is returned, and no attempt is made to initiate the transfer.
    ///
    /// This method is usually called by drivers, not by application code.
    pub fn control_in(&mut self, dev_addr: Option<DeviceAddress>, pipe_id: Option<PipeId>, setup: SetupPacket) -> Result<(), ControlError> {
        self.validate_control_pipe(dev_addr, pipe_id)?;
        if self.active_transfer.is_some() {
            return Err(ControlError::WouldBlock)
        }

        self.active_transfer = Some((pipe_id, transfer::Transfer::new_control_in(setup.length)));
        self.bus.set_recipient(dev_addr, 0, TransferType::Control);
        self.bus.write_setup(setup);

        Ok(())
    }

    /// Initiate an OUT transfer on the control endpoint of the given device
    ///
    /// If a `pipe_id` is given, the driver that set up the pipe will be able to associate the [`driver::Driver::completed_control`]
    /// call with this transfer.
    /// Otherwise the transfer will not be reported to any drivers.
    ///
    /// The `length` of the `setup` packet MUST be equal to the size of the `data` slice.
    ///
    /// If there is currently a transfer in progress, [`ControlError::WouldBlock`] is returned, and no attempt is made to initiate the transfer.
    ///
    /// This method is usually called by drivers, not by application code.
    pub fn control_out(&mut self, dev_addr: Option<DeviceAddress>, pipe_id: Option<PipeId>, setup: SetupPacket, data: &[u8]) -> Result<(), ControlError> {
        self.validate_control_pipe(dev_addr, pipe_id)?;

        if self.active_transfer.is_some() {
            return Err(ControlError::WouldBlock)
        }

        self.active_transfer = Some((pipe_id, transfer::Transfer::new_control_out(data.len() as u16)));
        self.bus.set_recipient(dev_addr, 0, TransferType::Control);
        self.bus.prepare_data_out(data);
        self.bus.write_setup(setup);

        Ok(())
    }

    fn validate_control_pipe(&self, dev_addr: Option<DeviceAddress>, pipe_id: Option<PipeId>) -> Result<(), ControlError> {
        let is_valid = match (dev_addr, pipe_id) {
            (None, None) | (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(given_dev_addr), Some(pipe_id)) => {
                // Index safety: a PipeId that is not in the 0..MAX_PIPES range (valid indices for self.pipes)
                //   should not be produced and indicates a bug within UsbHost.
                if let Some(Pipe::Control { dev_addr }) = self.pipes[pipe_id.0 as usize] {
                    dev_addr == given_dev_addr
                } else {
                    false
                }
            }
        };
        if is_valid {
            Ok(())
        } else {
            Err(ControlError::InvalidPipe)
        }
    }

    /// Initiate a `Get_Descriptor` (0x06) control IN transfer
    ///
    /// This is a convenience wrapper around [`UsbHost::control_in`], for the `Get_Descriptor` standard request.
    ///
    /// The `descriptor_type` can be one of the `TYPE_*` constants defined in the [`descriptor`] module, but usally these
    /// are already requested during the discovery phase.
    ///
    /// Thus usually this method will be used to request class- or vendor-specific descriptors.
    pub fn get_descriptor(
        &mut self,
        dev_addr: Option<DeviceAddress>,
        recipient: Recipient,
        descriptor_type: u8,
        descriptor_index: u8,
        length: u16
    ) -> Result<(), ControlError> {
        self.control_in(dev_addr, None, SetupPacket::new(
            UsbDirection::In,
            RequestType::Standard,
            recipient,
            Request::GET_DESCRIPTOR,
            ((descriptor_type as u16) << 8) | (descriptor_index as u16),
            0,
            length,
        ))
    }

    /// Initiate a `Set_Address` (0x05) control OUT transfer
    ///
    /// Private, since this is only used by the enumeration process.
    ///
    /// If drivers want to mess with the device address, they can do so manually.
    fn set_address(&mut self, address: DeviceAddress) -> Result<(), ControlError> {
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

    /// Initiate a `Set_Configuration` (0x09) control OUT transfer
    ///
    /// This is a convenience wrapper around [`UsbHost::control_out`] for the `Set_Configuration` standard request.
    ///
    /// Normally this does not need to be called manually. Instead the configuration is selected by the usb host during the discovery phase,
    /// depending on the drivers.
    ///
    /// Changing the configuration after the discovery phase is not supported yet by the driver interface. While it will probably work, make sure
    /// your drivers are aware of it and can handle this situation.
    pub fn set_configuration(&mut self, address: DeviceAddress, configuration: u8) -> Result<(), ControlError> {
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
    ///
    /// The given list of drivers must be the same on every call to `poll`, otherwise drivers will likely not function as intended.
    ///
    /// 
    pub fn poll(&mut self, drivers: &mut [&mut dyn driver::Driver<B>]) -> PollResult {
        let event = if let Some(event) = self.bus.poll() {
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
            Event::None
        };

        match &self.state {
            State::Enumeration(enumeration_state) => {
                match enumeration::process_enumeration(event, *enumeration_state, self) {
                    EnumerationState::Assigned(speed, dev_addr) => {
                        for driver in drivers {
                            driver.attached(dev_addr, speed);
                        }
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
                match discovery::process_discovery(event, dev_addr, *discovery_state, drivers, self) {
                    DiscoveryState::Done => {
                        let mut chosen_config = None;
                        // Ask all the drivers to choose a configuration
                        for driver in drivers {
                            if let Some(config) = driver.configure(dev_addr) {
                                // first driver to choose one wins...
                                chosen_config = Some(config);
                                // ...drivers later in the list don't get a say.
                                break;
                            }
                        }
                        if let Some(config) = chosen_config {
                            // Unwrap safety: when reaching `Done` state, the discovery phase leaves the bu sidle.
                            self.set_configuration(dev_addr, config).ok().unwrap();
                            self.state = State::Configuring(dev_addr, config);
                        } else {
                            self.state = State::Dormant(dev_addr);
                        }
                    }
                    DiscoveryState::ParseError => {
                        self.state = State::Dormant(dev_addr);
                        return PollResult::DiscoveryError(dev_addr);
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
                        for driver in drivers {
                            driver.configured(dev_addr, config, self);
                        }
                        self.state = State::Configured(dev_addr, config);
                    }
                    Event::Detached => {
                        for driver in drivers {
                            driver.detached(dev_addr);
                        }
                        self.reset();
                    }
                    _ => {}
                }
            }

            State::Configured(dev_addr, _config) => {
                match event {
                    Event::Detached => {
                        for driver in drivers {
                            driver.detached(*dev_addr);
                        }
                        self.reset();
                    }

                    Event::ControlInData(pipe_id, len) => {
                        if let Some(pipe_id) = pipe_id {
                            let data = unsafe { self.bus.control_buffer(len as usize) };
                            for driver in drivers {
                                driver.completed_control(*dev_addr, pipe_id, Some(data));
                            }
                        }
                    },

                    Event::ControlOutComplete(pipe_id) => {
                        if let Some(pipe_id) = pipe_id {
                            for driver in drivers {
                                driver.completed_control(*dev_addr, pipe_id, None);
                            }
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
                                    for driver in drivers {
                                        driver.completed_in(dev_addr, pipe_id, buf);
                                    }
                                },
                                UsbDirection::Out => {
                                    let buf = unsafe { core::slice::from_raw_parts_mut(ptr, size as usize) };
                                    for driver in drivers {
                                        driver.completed_out(dev_addr, pipe_id, buf);
                                    }
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
                        for driver in drivers {
                            driver.detached(*dev_addr);
                        }
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
