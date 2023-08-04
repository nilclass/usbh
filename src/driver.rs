use crate::types::{DeviceAddress, ConnectionSpeed};
use crate::bus::HostBus;
use crate::{UsbHost, PipeId};

pub mod kbd;
pub mod log;

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
    /// it should return that configuration's value ([`usbh::descriptor::ConfigurationDescriptor::value`]).
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
