use crate::types::{ConnectionSpeed, SetupPacket, DeviceAddress, TransferType};
use fugit::MillisDuration;
use defmt::Format;
use usb_device::UsbDirection;

pub trait HostBus {
    /// Reset the controller into it's initial state.
    fn reset_controller(&mut self);

    /// Reset the bus, but keep the controller initialized.
    ///
    /// The goal here is that communication with the device is interrupted, and it will show up as `Attached` again.
    fn reset_bus(&mut self);

    /// Enable sending SOF (for full-speed) or keep-alive (for low-speed) packets
    ///
    /// This prevents the attached device from entering suspend mode.
    fn enable_sof(&mut self);

    /// Check if SOF packets are currently enabled
    fn sof_enabled(&self) -> bool;

    /// Set device address and endpoint for future communication
    ///
    /// A `dev_addr` of `0` is represented as `None`.
    fn set_recipient(&mut self, dev_addr: Option<DeviceAddress>, endpoint: u8, transfer_type: TransferType);

    /// Write a SETUP packet to the bus
    fn write_setup(&mut self, setup: SetupPacket);

    /// Write a DATA IN packet to the bus, and receive `length`
    fn write_data_in(&mut self, length: u16, pid: bool);

    /// Write a DATA OUT packet to the bus, after loading the given `data` into the output buffer
    fn write_data_out(&mut self, data: &[u8]) {
        self.prepare_data_out(data);
        self.write_data_out_prepared();
    }

    /// Load the given `data` into the output buffer
    fn prepare_data_out(&mut self, data: &[u8]);

    /// Write a DATA OUT packet to the bus, assuming the buffers were already prepared
    fn write_data_out_prepared(&mut self);

    fn poll(&mut self) -> PollResult;

    fn process_received_data<F: FnOnce(&[u8]) -> T, T>(&self, f: F) -> T;

    unsafe fn control_buffer(&self, len: usize) -> &[u8];

    fn create_interrupt_pipe(&mut self, device_address: DeviceAddress, endpoint_number: u8, direction: UsbDirection, size: u16, interval: u8) -> Option<(*mut u8, u8)>;

    fn release_interrupt_pipe(&mut self, pipe_ref: u8);

    fn received_len(&self) -> u16;

    fn dump_dpram(&self);

    fn pipe_buf(&self, pipe_index: u8) -> &[u8];

    fn pipe_continue(&self, pipe_index: u8);

    /// Enable/disable interrupt on SOF
    ///
    /// While enabled, the host bus should generate (call `poll` on the hsot) whenever
    /// a start-of-frame is sent.
    /// This is used by the enumeration process to implement wait times.
    ///
    /// If the controller does not support SOF interrupts natively, they can be implemented
    /// with a platform-specific timer.
    fn interrupt_on_sof(&mut self, enable: bool);
}

pub struct PollResult {
    pub event: Option<Event>,
    pub poll_again_after: Option<MillisDuration<u8>>,
}

#[derive(Copy, Clone, Format, PartialEq)]
pub enum Event {
    /// A new device was attached, with given speed
    Attached(ConnectionSpeed),
    /// The device is no longer attached
    Detached,
    /// A control transaction (SETUP, DATA IN or DATA OUT) has completed
    TransComplete,
    /// Device sent a STALL. This usually means that the device does not understand our communication
    Stall,
    /// Device has resumed from sleep?
    Resume,
    /// An error has occured (details in the Error)
    Error(Error),
    /// Data from interrupt pipe is available to be read or written
    InterruptPipe(u8),
    Sof,
}

#[derive(Copy, Clone, Format, PartialEq)]
pub enum Error {
    /// CRC mismatch
    Crc,
    /// Bit stuffing rules were not followed
    BitStuffing,
    /// Data was received faster than it could be processed
    RxOverflow,
    /// Expected data to be received, but it did not arrive in time
    RxTimeout,
    /// Data sequence error. Saw DATA0 when expecting DATA1 or vice versa.
    DataSequence,
    /// None of the above. Hardware specific error condition.
    Other,
}
