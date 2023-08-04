//! Interface for host bus hardware
//!
//! In order to use `usbh` on a given device, there must be a [`bus::HostBus`] implementation.
//!
//! This interface is still evolving, as there is only one (partially complete) implementation so far.
//!

use crate::types::{ConnectionSpeed, SetupPacket, DeviceAddress, TransferType};
use fugit::MillisDuration;
use defmt::Format;
use usb_device::UsbDirection;

pub trait HostBus {
    /// Reset the controller into it's initial state.
    ///
    /// This is called once as the UsbHost is initialized, and will be called again when [`crate::UsbHost::reset`] is called.
    ///
    /// It must do any necessary preparation needed to enable the hardware and put it into the appropriate mode to act as a host.
    ///
    /// It must also reset any internal state related to this HostBus interface to a default configuration.
    ///
    /// If applicable, this is also the point where all interrupts should be enabled that are necessary to generate the
    /// appropriate [`Event`]s when `poll` is called.
    ///
    /// This method must *not* enable interrupts on start-of-frame. SOF-interrupts are separately controlled by [`HostBus::interrupt_on_sof`].
    fn reset_controller(&mut self);

    /// Reset the bus, but keep the controller initialized.
    ///
    /// Must cause a RESET condition on the bus.
    ///
    /// Must not disable any interrupts previously set up, but may suspend generating SOF / keep-alive packets, requiring the host to
    /// call [`HostBus::enable_sof`] after the reset is complete.
    fn reset_bus(&mut self);

    /// Enable sending SOF (for full-speed) or keep-alive (for low-speed) packets
    ///
    /// This prevents the attached device from entering suspend mode.
    fn enable_sof(&mut self);

    /// Check if SOF packets are currently enabled
    fn sof_enabled(&self) -> bool;

    /// Set device address, endpoint and transfer type for an upcoming transfer
    ///
    /// A `dev_addr` of `0` is represented as `None`.
    ///
    /// This method is always called before a transfer is initiated. It must have effect for all future transactions (`SETUP`, `DATA`, ...),
    /// until `set_recipient` is called again.
    fn set_recipient(&mut self, dev_addr: Option<DeviceAddress>, endpoint: u8, transfer_type: TransferType);

    /// Write a SETUP packet to the bus
    ///
    /// Once the packet has been acknowledged by the device, a [`Event::TransComplete`] must be generated.
    ///
    /// This method must not modify the buffers used for DATA transfers.
    /// In particular if [`prepare_data_out`] is called before [`write_setup`], as soon as [`Event::TransComplete`]
    /// occurs, the data buffer must be in the prepared state, and ready for a [`write_data_out_prepared`] call.
    fn write_setup(&mut self, setup: SetupPacket);

    /// Write a DATA IN packet to the bus, then receive `length` bytes
    ///
    /// Once all data has been received, a [`Event::TransComplete`] must be generated.
    fn write_data_in(&mut self, length: u16, pid: bool);

    /// Write a DATA OUT packet to the bus, after loading the given `data` into the output buffer
    ///
    /// Once all data has been sent, a [`Event::TransComplete`] must be generated.
    fn write_data_out(&mut self, data: &[u8]) {
        self.prepare_data_out(data);
        self.write_data_out_prepared();
    }

    /// Load the given `data` into the output buffer
    ///
    /// After this method was called, a [`write_data_out_prepared`] call should write this data.
    ///
    /// The prepared data should be overwritten by any future call to [`prepare_data_out`], [`write_data_in`] or [`write_data_out`].
    ///
    /// In other words: the data buffer can be shared by IN and OUT transfers, since there will only ever be one of them in progress at any time.
    fn prepare_data_out(&mut self, data: &[u8]);

    /// Write a DATA OUT packet to the bus, assuming the buffers were already prepared
    ///
    /// The data sent will have been passed to [`prepare_data_out`] before this call.
    ///
    /// Once all data has been sent, a [`Event::TransComplete`] must be generated.
    fn write_data_out_prepared(&mut self);

    /// Check if there is an event pending on the bus, if there is return it.
    fn poll(&mut self) -> Option<Event>;

    unsafe fn control_buffer(&self, len: usize) -> &[u8];

    fn create_interrupt_pipe(&mut self, device_address: DeviceAddress, endpoint_number: u8, direction: UsbDirection, size: u16, interval: u8) -> Option<(*mut u8, u8)>;

    fn release_interrupt_pipe(&mut self, pipe_ref: u8);

    fn received_len(&self) -> u16;

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
