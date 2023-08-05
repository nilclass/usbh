//! Interface for implementing drivers
//!
//! Drivers are instantiated by application code and passed to the [`UsbHost::poll`](crate::UsbHost::poll) function.
//!
//! The methods defined in this trait are then called by by the UsbHost at the appropriate times.
//!
//! All of these methods are called on *all* of the drivers (with the exception fo the [`configure`](Driver::configure) method).
//!
//! Drivers must interpret the `dev_addr` parameter (and `pipe_id` where applicable) to determine if the event
//! targets one of the devices that the driver is controlling.
//!
//! In general multiple drivers can communicate with the same device, except that only one driver can decide which device
//! configuration to set.
//!
//! ## Walkthrough for a newly connected device
//!
//! 1. Initially the device has no address, so the host enters **enumeration**
//! 2. When enumeration has succeeded, the host calls [`attached`](Driver::attached), informing drivers that a new device is available, and enters **discovery** phase
//! 3. During discovery, the host requests the *device descriptor* from the device, and subsequently requests the *configuration descriptor* for each of
//!    the configurations that the device supports. All of these descriptors are parsed into `descriptor_type` and `data` and passed to the [`descriptor`](Driver::descriptor) method one-by-one.
//!    When requesting a configuration descriptor, the device sends *all* of the nested descriptors (interface, endpoint, class specifics, ...) as well.
//!    The discovery logic separates these descriptors and passes each of them to the [`descriptor`](Driver::descriptor) method separately.
//! 4. When all descriptors have been fetched, the host enters the **configuration** phase.
//! 5. During configuration, the host calls [`configure`](Driver::configure) on each of the drivers *until one of them returns a value*.
//!    The value must be a valid configuration value (i.e. come from a [`ConfigurationDescriptor::value`](crate::descriptor::ConfigurationDescriptor::value)).
//! 6. If all of the drivers' `configure` calls returned `None` (no driver is interested in it), the host enteres **dormant** state.
//!    Otherwise the host calls [`configured`](Driver::configured) on *all* of the drivers and enteres **configured** state.
//! 7. The [`configured`](Driver::configured) callback informs the driver about the chosen configuration, and gives access to the host interface,
//!    to allow the driver to set up pipes for the device's endpoints.
//!    Currently only **control pipes** and **interrupt pipes** are supported.
//!
//! This concludes the configuration phase. If the device ends up in **configured** state (one of the drivers selected a configuration),
//! drivers can communicate with the device from now on.
//!
//! ## Communicating with the device
//!
//! Drivers cannot initiate communication through the host on their own, they must be given access to the `UsbHost` instance by application code to do so.
//!
//! For this purpose the driver should define function specific methods on the driver object, which take a `&mut UsbHost<B>`.
//!
//! Communication may only happen through the pipes which were created by the [`Driver::configured`] callback. Pipes are identified by a [`PipeId`].
//!
//! ### Control transfers
//!
//! To initiate a control transfer, the driver must have created a control pipe ([`create_control_pipe`](crate::UsbHost::create_control_pipe)).
//!
//! That pipe can then be passed to [`control_in`](crate::UsbHost::control_in) or [`control_out`](crate::UsbHost::control_out), to initiate a transfer.
//!
//! Example:
//! ```ignore
//! // our driver keeps track of only one device, and a single pipe
//! struct MyDriver {
//!     dev_addr: Option<DeviceAddress>,
//!     control_pipe: Option<PipeId>,
//! }
//!
//! impl<B: HostBus> Driver<B> for MyDriver {
//!     fn configured(&mut self, dev_addr: DeviceAddress, _value: u8, host: &mut UsbHost) {
//!         self.dev_addr = Some(dev_addr);
//!         // NOTE: the host can only handle a fixed number of pipes. If it runs out of pipes, None is returned.
//!         self.control_pipe = host.create_control_pipe(dev_addr);
//!     }
//!
//!     // remaining methods omitted for brevity...
//! }
//!
//! impl MyDriver {
//!     // driver specific method, which will be called by application code
//!     fn turn_on_led<B: HostBus>(&mut self, host: &mut UsbHost<B>) -> Result<(), ControlError> {
//!         if let (Some(dev_addr), Some(control_pipe)) = (self.dev_addr, self.control_pipe) {
//!             host.control_out(
//!                 // device being addressed
//!                 Some(dev_addr),
//!                 // control pipe to use
//!                 Some(control_pipe),
//!                 // setup packet (function specific)
//!                 SetupPacket::new(UsbDirection::Out, /* ... */),
//!                 // data to send (function specific)
//!                 &[/* ... */],
//!             )?;
//!         }
//!         Ok(())
//!     }
//! }
//! ```
//!
//!
//!
use crate::bus::HostBus;
use crate::types::{ConnectionSpeed, DeviceAddress};
use crate::{PipeId, UsbHost};

pub mod kbd;
pub mod log;

/// The Driver trait
///
/// See [module-level documentation](`crate::driver`) for details.
///
pub trait Driver<B: HostBus> {
    /// New device was attached, and got assigned the given address.
    ///
    /// This is where the driver can set up internal structures to continue processing the device.
    fn attached(&mut self, dev_addr: DeviceAddress, connection_speed: ConnectionSpeed);

    /// The device with the given address was detached.
    ///
    /// Clean up any internal data related to the device here.
    fn detached(&mut self, dev_addr: DeviceAddress);

    /// A descriptor was received for the device
    ///
    /// When a new device is attached, the device descriptor and all the configuration descriptors will
    /// be requested by the enumeration process and fed to all of the drivers.
    ///
    /// The driver should parse these descriptors to figure out if it can handle a given device or not.
    fn descriptor(&mut self, dev_addr: DeviceAddress, descriptor_type: u8, data: &[u8]);

    /// The host is asking the driver to configure the device.
    ///
    /// If the driver can handle one of the configurations of the device (based on the descriptor),
    /// it should return that configuration's value ([`crate::descriptor::ConfigurationDescriptor::value`]).
    ///
    /// Otherwise it should return None.
    ///
    /// This method is called on each of the drivers, until the first one succeeds.
    fn configure(&mut self, dev_addr: DeviceAddress) -> Option<u8>;

    /// Informs the driver that a given configuration was selected for this device.
    ///
    /// Here the driver can set up pipes for the device's endpoints.
    fn configured(&mut self, dev_addr: DeviceAddress, value: u8, host: &mut UsbHost<B>);

    /// Called when a control transfer was completed on the given pipe
    ///
    /// For IN transfers, `data` contains the received data, for OUT transfers it is `None`.
    fn completed_control(&mut self, dev_addr: DeviceAddress, pipe_id: PipeId, data: Option<&[u8]>);

    /// Called when data was received on the given IN pipe
    fn completed_in(&mut self, dev_addr: DeviceAddress, pipe_id: PipeId, data: &[u8]);

    /// Called when new data is needed for the given OUT pipe
    fn completed_out(&mut self, dev_addr: DeviceAddress, pipe_id: PipeId, data: &mut [u8]);
}
