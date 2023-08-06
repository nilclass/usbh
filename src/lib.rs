//! Experimental host-side USB stack for embedded devices.
//!
//! `usbh` aims to abstract between two things:
//!  
//! - embedded USB host controllers on one side
//! - function specific USB drivers on the other side
//!  
//! The goal is that a driver can be developed independently from the specific host controller hardware, similarly to the `usb-device` crate allowing implementing USB functions independently of the USB device controller.
//!  
//! ## Implementing drivers
//!
//! Please check out the [documentation for the `driver` module](crate::driver).
//!
//! ## Adding support for new hardware
//!
//! Since this project is in an early stage, this area is largely unexplored.
//!
//! The [`bus` module](crate::bus) contains a `HostBus` trait, which must be implemented for the target hardware.
//!
//! If you are planning to implement support for a new hardware, please [open an issue](https://github.com/nilclass/usbh/issues/new?title=Hardware%20support%20for:%20...), so we can figure out together if any additions or changes to the HostBus interface are necessary to accomodate it.
//!
//! ## Usage
//!
//! The code block below shows a brief example, leaving out any hardware and driver specifics.
//!
//! For a full example, please check out the [rp-hal-usb-host-example](https://github.com/nilclass/rp-hal-usb-host-example) which contains a runnable example, targeting RP2040 based boards.
//!
//! ```ignore
//! use usbh::{
//!     UsbHost,
//!     PollResult,
//! };
//!
//! fn main() {
//!     // must implement usbh::bus::HostBus
//!     let host_bus = hardware_specific_host_bus_initialization();
//!
//!     let usb_host = UsbHost::new(host_bus);
//!
//!     // these must implement usbh::driver::Driver;
//!     let mut driver1 = create_first_driver();
//!     let mut driver2 = create_second_driver();
//!
//!     // (leaving out details on how to share `usb_host` and `driver*` with the interrupt routine)
//! }
//!
//! #[...]
//! fn USB_IRQ() {
//!     match usb_host.poll(&mut [&mut driver1, &mut driver2]) {
//!         PollResult::BusError(error) => {
//!             // something went wrong
//!         }
//!         PollEvent::DiscoveryError(device_address) => {
//!             // device with specified address misbehaved during discovery (it will likely not be usable)
//!         }
//!         PollResult::NoDevice => {
//!             // no device is currently connected
//!         }
//!         Event::Busy => {
//!             // Host is currently busy with a transfer.
//!             // Trying to start a transfer now will return `WouldBlock`.
//!         }
//!         Event::Idle => {
//!             // Host is not currently handling any transfer.
//!             // A new transfer can be started, if desired.
//!         }
//!     }
//!
//!     // After polling the USB host, the drivers may have new things to report:
//!     if let Some(event) = driver1.take_event() {
//!        // ...
//!     }
//!     if driver2.something_something() {
//!        // ...
//!     }
//! }
//! ```

#![no_std]

use embed_doc_image::embed_doc_image;

pub mod bus;
pub mod driver;
pub mod types;

mod discovery;
mod enumeration;
mod transfer;

pub mod descriptor;

use bus::HostBus;
use core::num::NonZeroU8;
use defmt::Format;
use discovery::DiscoveryState;
use enumeration::EnumerationState;
use types::{DeviceAddress, SetupPacket, TransferType};
use usb_device::{
    control::{Recipient, Request, RequestType},
    UsbDirection,
};

/// Maximum number of pipes that the host supports.
const MAX_PIPES: usize = 32;

/// State of the host stack
///
/// Currently the host can only handle a single port, with a single device.
/// When that changes, this state will need to be split, to be per-host / per-port / per-device, as needed.
#[derive(Copy, Clone)]
enum State {
    /// Enumeration phase: starts in WaitForDevice state, ends with an address being assigned
    Enumeration(EnumerationState),
    /// Discovery phase: starts with an assigned address, ends with a configuration being chosen
    Discovery(DeviceAddress, DiscoveryState),
    /// Configuration phase: put the device into the chosen configuration
    Configuring(DeviceAddress, u8),
    /// The device is configured. Communication is forwarded to drivers.
    Configured(DeviceAddress, u8),
    /// No driver is interested, or the device misbehaved during one of the previous phases
    Dormant(DeviceAddress),
}

/// Error initiating a control transfer
#[derive(Copy, Clone, PartialEq)]
pub enum ControlError {
    /// Indicates that the bus is currently busy with another transfer.
    ///
    /// The transfer can be tried again once the host's `poll` method returned [`PollResult::Idle`].
    WouldBlock,

    /// A control transfer was initiated using an invalid `PipeId`.
    ///
    /// This could indicate a bug in the driver (the driver held on to a pipe handle after the corresponding device was detached),
    /// or a bug in application code (e.g. if the host was [`reset`](UsbHost::reset) without re-initializing all drivers).
    InvalidPipe,
}

/// Internal event type, used by `poll` and the enumeration process
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
#[non_exhaustive]
pub enum PollResult {
    /// There is no device attached. It does not make sense to do anything else with the UsbHost instance, until a device was attached.
    NoDevice,

    /// Bus is currently busy talking to a device. Calling any transfer methods on the host will result in [`ControlError::WouldBlock`].
    Busy,

    /// A device is attached and the bus is available. The caller can use the UsbHost instance to start a transfer.
    Idle,

    /// The host bus encountered an error
    BusError(bus::Error),

    /// An error happened during discovery.
    ///
    /// After this result the host is put in "dormant" state until the device is removed.
    DiscoveryError(DeviceAddress),
}

/// Entrypoint for the USB host stack
///
/// The `UsbHost` type is the core of the host stack, implementing various state machines to facilitate:
/// - control transfers
/// - enumeration process: assigning an address to a device
/// - discovery: query descriptors of the device (delivered to drivers)
/// - configuration: select a configuration (controlled by drivers)
///
/// After a device is configured, the UsbHost facilitates communication between drivers and the host bus.
///
/// ## Host stack phases
///
/// At any time, the UsbHost is in one of **five phases**. **Four** of these are shown in this diagram:
///
/// ![Diagram showing enumeration, discovery, configuration and configured phase, and their transitions][usb-host-phases]
///
/// When a new device is connected to the host, the host moves through these phases, ending up either in the *configured* stage (at which point drivers
/// can take over communication with the device), or in the *dormant* phase (not shown), if no driver is interested in the device.
///
/// If there is an error communicating with the device in any of the previous stages, the device also ends up in the dormant phase.
///
/// Finally, if the device is disconnected, regardless of the current phase, the host returns to the Enumeration phase
/// (there is one exception to this: within the enumeration phase, two resets are performed, during which the device will
/// "disconnect" and "connect" again - these disconnects do not return to the initial enumeration state).
///
/// For a more detailed description of these phases, check out the [documentation for the Driver interface](crate::driver).
///
#[embed_doc_image("usb-host-phases", "doc/usb-host-phases.png")]
pub struct UsbHost<B> {
    bus: B,
    state: State,
    active_transfer: Option<(Option<PipeId>, transfer::Transfer)>,
    last_address: u8,
    pipes: [Option<Pipe>; MAX_PIPES],
}

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
    },
}

unsafe impl Send for Pipe {}

/// Handle for a pipe
///
/// A pipe connects a specific endpoint of a specific device to a driver.
#[derive(Copy, Clone, PartialEq, Format)]
pub struct PipeId(u8);

impl<B: HostBus> UsbHost<B> {
    /// Initialize the USB host stack
    ///
    /// Resets the `HostBus` controller using [`reset_controller`](bus::HostBus::reset_controller).
    ///
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

    /// Poll the USB host. This must be called reasonably often.
    ///
    /// If the host implementation has an interrupt that fires on USB activity, then calling it once in that interrupt handler is enough.
    /// Otherwise make sure to call it at least once per millisecond.
    ///
    /// The given list of drivers must be the same on every call to `poll`, otherwise drivers will likely not function as intended.
    ///
    /// ```ignore
    /// #[...]
    /// fn USB_IRQ() {
    ///     match usb_host.poll(&mut [&mut driver_1, &mut driver_2, ...]) {
    ///        ...
    ///     }
    /// }
    /// ```
    pub fn poll(&mut self, drivers: &mut [&mut dyn driver::Driver<B>]) -> PollResult {
        let event = if let Some(event) = self.bus.poll() {
            match event {
                bus::Event::Attached(speed) => Event::Attached(speed),
                bus::Event::Detached => Event::Detached,
                bus::Event::TransComplete => {
                    if let Some((pipe_id, transfer)) = self.active_transfer.take() {
                        match transfer.stage_complete(self) {
                            transfer::PollResult::ControlInComplete(length) => {
                                Event::ControlInData(pipe_id, length)
                            }
                            transfer::PollResult::ControlOutComplete => {
                                Event::ControlOutComplete(pipe_id)
                            }
                            transfer::PollResult::Continue(transfer) => {
                                self.active_transfer = Some((pipe_id, transfer));
                                Event::None
                            }
                        }
                    } else {
                        panic!("BUG: received WriteComplete while no transfer was in progress")
                    }
                }
                bus::Event::Resume => {
                    // TODO: figure out if drivers need to see this event
                    Event::Resume
                }
                bus::Event::Stall => {
                    // TODO: figure out if we should reset everything in case of a stall, or just ignore it until the device is unplugged.
                    // Notifying the drivers and the application of this condition would also make sense.
                    Event::Stall
                }
                bus::Event::Error(error) => Event::BusError(error),
                bus::Event::InterruptPipe(buf_ref) => Event::InterruptPipe(buf_ref),
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
                match discovery::process_discovery(event, dev_addr, *discovery_state, drivers, self)
                {
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
                            // Unwrap safety: when reaching `Done` state, the discovery phase leaves the bus idle.
                            self.set_configuration(dev_addr, None, config).ok().unwrap();
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

            State::Configured(dev_addr, _config) => match event {
                Event::Detached => {
                    for driver in drivers {
                        driver.detached(*dev_addr);
                    }
                    self.cleanup(*dev_addr);
                }

                Event::ControlInData(pipe_id, len) => {
                    if let Some(pipe_id) = pipe_id {
                        let data = self.bus.received_data(len as usize);
                        for driver in drivers {
                            driver.completed_control(*dev_addr, pipe_id, Some(data));
                        }
                    }
                }

                Event::ControlOutComplete(pipe_id) => {
                    if let Some(pipe_id) = pipe_id {
                        for driver in drivers {
                            driver.completed_control(*dev_addr, pipe_id, None);
                        }
                    }
                }

                Event::InterruptPipe(pipe_ref) => {
                    let matching_pipe = self
                        .pipes
                        .iter()
                        .enumerate()
                        .find(|(_, pipe)| {
                            if let Some(Pipe::Interrupt { bus_ref, .. }) = pipe {
                                *bus_ref == pipe_ref
                            } else {
                                false
                            }
                        })
                        .map(|(id, pipe)| (PipeId(id as u8), pipe.unwrap()));

                    if let Some((
                        pipe_id,
                        Pipe::Interrupt {
                            dev_addr,
                            size,
                            ptr,
                            direction,
                            ..
                        },
                    )) = matching_pipe
                    {
                        match direction {
                            UsbDirection::In => {
                                let buf =
                                    unsafe { core::slice::from_raw_parts(ptr, size as usize) };
                                for driver in drivers {
                                    driver.completed_in(dev_addr, pipe_id, buf);
                                }
                            }
                            UsbDirection::Out => {
                                let buf =
                                    unsafe { core::slice::from_raw_parts_mut(ptr, size as usize) };
                                for driver in drivers {
                                    driver.completed_out(dev_addr, pipe_id, buf);
                                }
                            }
                        }
                    }
                    self.bus.pipe_continue(pipe_ref);
                }

                Event::BusError(error) => return PollResult::BusError(error),

                _ => {}
            },

            State::Dormant(dev_addr) => match event {
                Event::Detached => {
                    for driver in drivers {
                        driver.detached(*dev_addr);
                    }
                    self.reset();
                }
                _ => {}
            },
        }

        if let State::Enumeration(EnumerationState::WaitForDevice) = self.state {
            PollResult::NoDevice
        } else if self.active_transfer.is_some() {
            PollResult::Busy
        } else {
            PollResult::Idle
        }
    }

    /// Reset the entire host stack
    ///
    /// This resets the host controller (via [`bus::HostBus::reset_controller`]) and resets
    /// all internal state of the UsbHost to their defaults.
    ///
    /// Any current transfer will never complete, and any pipes created will no longer be valid.
    /// At the end of the reset, no device will be connected.
    ///
    /// **Drivers must never call this method.**
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

    fn alloc_pipe(&mut self) -> Option<(PipeId, &mut Option<Pipe>)> {
        self.pipes
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
            .map(|(i, slot)| (PipeId(i as u8), slot))
    }

    /// Create a pipe for control transfers
    ///
    /// This method is meant to be called by drivers.
    ///
    /// The returned `PipeId` can be used to initiate transfers by calling [`control_out`](UsbHost::control_out),
    /// [`control_in`](UsbHost::control_in) or one of their wrappers.
    ///
    /// Returns `None` if the maximum number of supported pipes has been reached.
    pub fn create_control_pipe(&mut self, dev_addr: DeviceAddress) -> Option<PipeId> {
        self.alloc_pipe().map(|(id, slot)| {
            slot.replace(Pipe::Control { dev_addr });
            id
        })
    }

    /// Returns the next unassigned address, and increments the counter
    ///
    /// The address is allowed to overflow, at which point it starts out at 1 again (0 is skipped).
    ///
    /// FIXME: prevent re-use of addresses. The overflowing address counter is not just theoretical,
    ///   it can be triggered by a device resetting itself over and over directly after receiving an address.
    fn next_address(&mut self) -> DeviceAddress {
        self.last_address = self.last_address.wrapping_add(1);
        if self.last_address == 0 {
            self.last_address += 1;
        }
        DeviceAddress(NonZeroU8::new(self.last_address).unwrap())
    }

    /// Initiate an IN transfer on the control endpoint of the given device
    ///
    /// If a `pipe_id` is given, the driver that set up the pipe will be able to associate the subsequent
    /// [`completed_control`](driver::Driver::completed_control) callback with this transfer.
    /// Otherwise the transfer will not be reported to any drivers.
    ///
    /// The number of bytes transferred is determined by the `length` from the setup packet.
    ///
    /// If there is currently a transfer in progress, [`ControlError::WouldBlock`] is returned, and no attempt is made to initiate the transfer.
    ///
    /// This method is usually called by drivers, not by application code.
    pub fn control_in(
        &mut self,
        dev_addr: Option<DeviceAddress>,
        pipe_id: Option<PipeId>,
        setup: SetupPacket,
    ) -> Result<(), ControlError> {
        self.validate_control_pipe(dev_addr, pipe_id)?;
        if self.active_transfer.is_some() {
            return Err(ControlError::WouldBlock);
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
    pub fn control_out(
        &mut self,
        dev_addr: Option<DeviceAddress>,
        pipe_id: Option<PipeId>,
        setup: SetupPacket,
        data: &[u8],
    ) -> Result<(), ControlError> {
        self.validate_control_pipe(dev_addr, pipe_id)?;

        if self.active_transfer.is_some() {
            return Err(ControlError::WouldBlock);
        }

        self.active_transfer = Some((
            pipe_id,
            transfer::Transfer::new_control_out(data.len() as u16),
        ));
        self.bus.set_recipient(dev_addr, 0, TransferType::Control);
        self.bus.prepare_data_out(data);
        self.bus.write_setup(setup);

        Ok(())
    }

    fn validate_control_pipe(
        &self,
        dev_addr: Option<DeviceAddress>,
        pipe_id: Option<PipeId>,
    ) -> Result<(), ControlError> {
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
        pipe_id: Option<PipeId>,
        recipient: Recipient,
        descriptor_type: u8,
        descriptor_index: u8,
        length: u16,
    ) -> Result<(), ControlError> {
        self.control_in(
            dev_addr,
            pipe_id,
            SetupPacket::new(
                UsbDirection::In,
                RequestType::Standard,
                recipient,
                Request::GET_DESCRIPTOR,
                ((descriptor_type as u16) << 8) | (descriptor_index as u16),
                0,
                length,
            ),
        )
    }

    /// Initiate a `Set_Address` (0x05) control OUT transfer
    ///
    /// Private, since this is only used by the enumeration process.
    ///
    /// If drivers want to mess with the device address, they can do so manually.
    fn set_address(&mut self, address: DeviceAddress) -> Result<(), ControlError> {
        self.control_out(
            None,
            None,
            SetupPacket::new(
                UsbDirection::Out,
                RequestType::Standard,
                Recipient::Device,
                Request::SET_ADDRESS,
                address.into(),
                0,
                0,
            ),
            &[],
        )
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
    pub fn set_configuration(
        &mut self,
        dev_addr: DeviceAddress,
        pipe_id: Option<PipeId>,
        configuration: u8,
    ) -> Result<(), ControlError> {
        self.control_out(
            Some(dev_addr),
            pipe_id,
            SetupPacket::new(
                UsbDirection::Out,
                RequestType::Standard,
                Recipient::Device,
                Request::SET_CONFIGURATION,
                configuration as u16,
                0,
                0,
            ),
            &[],
        )
    }

    /// Create a pipe for interrupt transfers
    ///
    /// This method is meant to be called by drivers.
    ///
    /// Transfers on the interrupt pipe are always initiated by the host controller at the appropriate times.
    ///
    /// Drivers must implement the [`completed_in`](driver::Driver::completed_in) / [`completed_out`](driver::Driver::completed_out) callbacks to
    /// consume / produce data for the pipe as needed. The returned `PipeId` will be passed to those callbacks for the
    /// driver to be able to associate the calls with an individual pipe they created.
    ///
    /// Returns `None` if the maximum number of supported pipes has been reached.
    pub fn create_interrupt_pipe(
        &mut self,
        dev_addr: DeviceAddress,
        ep_number: u8,
        direction: UsbDirection,
        size: u16,
        interval: u8,
    ) -> Option<PipeId> {
        if let Some(bus::InterruptPipe { bus_ref, ptr }) = self.bus().create_interrupt_pipe(dev_addr, ep_number, direction, size, interval) {
            if let Some((id, slot)) = self.alloc_pipe() {
                slot.replace(Pipe::Interrupt {
                    dev_addr,
                    bus_ref,
                    direction,
                    size,
                    ptr,
                });
                Some(id)
            } else {
                self.bus().release_interrupt_pipe(bus_ref);
                // the host has no more free pipe slots
                None
            }
        } else {
            // the bus has no free interrupt pipes
            None
        }
    }

    pub fn bus(&mut self) -> &mut B {
        &mut self.bus
    }

    /// Clean up after device was removed
    fn cleanup(&mut self, addr: DeviceAddress) {
        for pipe in self.pipes.iter_mut() {
            match pipe {
                Some(Pipe::Control { dev_addr } | Pipe::Interrupt { dev_addr, .. })
                    if *dev_addr == addr =>
                {
                    *pipe = None;
                }
                _ => {}
            }
        }

        if self.active_transfer.is_some() {
            self.active_transfer.take();
        }
    }
}
