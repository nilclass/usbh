use crate::types::DeviceAddress;
use crate::bus::HostBus;
use crate::UsbHost;

pub trait Driver<B: HostBus> {
    /// New device was attached, and got assigned the given address.
    ///
    /// This is a good time to request some descriptors.
    fn attached(&mut self, device_address: DeviceAddress, host: &mut UsbHost<B>);

    /// The device with the given address was detached.
    ///
    /// Clean up any internal data related to the device here.
    fn detached(&mut self, device_address: DeviceAddress);

    fn transfer_in_complete(&mut self, device_address: DeviceAddress, length: usize, host: &mut UsbHost<B>);

    fn transfer_out_complete(&mut self, device_address: DeviceAddress, host: &mut UsbHost<B>);

    fn interrupt_in_complete(&mut self, device_address: DeviceAddress, length: usize, host: &mut UsbHost<B>);

    fn interrupt_out_complete(&mut self, device_address: DeviceAddress, host: &mut UsbHost<B>);

    fn pipe_event(&mut self, device_address: DeviceAddress, data: &[u8]);
}
