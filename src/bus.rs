//! Interface for host bus hardware
//!
//! In order to use `usbh` on a given device, there must be a [`HostBus`] implementation specific to that device.
//!
//! This interface is still evolving, as there is only one (partially complete) implementation so far.
//!

use crate::types::{ConnectionSpeed, DeviceAddress, SetupPacket, TransferType};
use defmt::Format;
use usb_device::UsbDirection;

/// Interface for host bus hardware
///
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
    fn set_recipient(
        &mut self,
        dev_addr: Option<DeviceAddress>,
        endpoint: u8,
        transfer_type: TransferType,
    );

    /// Write a SETUP packet to the bus
    ///
    /// Once the packet has been acknowledged by the device, a [`Event::TransComplete`] must be generated.
    ///
    /// This method must not modify the buffers used for DATA transfers.
    /// In particular if [`HostBus::prepare_data_out`] is called before [`HostBus::write_setup`], as soon as [`Event::TransComplete`]
    /// occurs, the data buffer must be in the prepared state, and ready for a [`HostBus::write_data_out_prepared`] call.
    fn write_setup(&mut self, setup: SetupPacket);

    /// Write a DATA IN packet to the bus, then receive `length` bytes
    ///
    /// Once all data has been received, a [`Event::TransComplete`] must be generated.
    ///
    /// Afterwards the received data must be accessible via [`received_data`](HostBus::received_data).
    fn write_data_in(&mut self, length: u16, pid: bool);

    /// Write a DATA OUT packet to the bus, after loading the given `data` into the output buffer
    ///
    /// Once all data has been sent, a [`Event::TransComplete`] must be generated.
    ///
    /// The default implementation is a wrapper around [`HostBus::prepare_data_out`] followed by [`HostBus::write_data_out_prepared`].
    fn write_data_out(&mut self, data: &[u8]) {
        self.prepare_data_out(data);
        self.write_data_out_prepared();
    }

    /// Load the given `data` into the output buffer
    ///
    /// After this method was called, a [`HostBus::write_data_out_prepared`] call should write this data.
    ///
    /// The prepared data may be overwritten by any future call to [`HostBus::prepare_data_out`], [`HostBus::write_data_in`] or [`HostBus::write_data_out`].
    ///
    /// In other words: the data buffer can be shared by IN and OUT transfers, since there will only ever be one of them in progress at any time.
    fn prepare_data_out(&mut self, data: &[u8]);

    /// Write a DATA OUT packet to the bus, assuming the buffers were already prepared
    ///
    /// The data sent will have been passed to [`HostBus::prepare_data_out`] before this call.
    ///
    /// Once all data has been sent, a [`Event::TransComplete`] must be generated.
    fn write_data_out_prepared(&mut self);

    /// Check if there is an event pending on the bus, if there is return it.
    ///
    /// This will be called whenever application code calls [`crate::UsbHost::poll`].
    fn poll(&mut self) -> Option<Event>;

    /// Access the input buffer for a recent transfer
    ///
    /// This method will be called after the host bus completed a DATA IN transfer, as signaled by `Event::TransComplete`.
    ///
    /// The given `length` will be equal to the `length` passed to the most recent `write_data_in` call.
    ///
    /// The returned buffer *should* be exactly `length` bytes long. It *may* also be smaller though, if `length` exceeds
    /// the maximum buffer size that the host bus supports.
    fn received_data(&self, length: usize) -> &[u8];

    /// Create an interrupt pipe
    ///
    /// Interrupt pipes are managed by the host bus.
    ///
    /// The lifecycle of an interrupt pipe is as follows:
    /// 1. `create_interrupt_pipe` (this method) is called to set up a pipe
    /// 2. the host bus generates [`Event::InterruptPipe`] events as appropriate:
    ///    - if the `direction` is `In`: the event is generated when the device has sent new data
    ///      (the device may reply with `NAK` any number of times while there is no data - this must not generate any events)
    ///    - if the `direction` is `Out`: the event is generated when the latest data has been sent out, and new data
    ///      can be placed in the buffer
    /// 3. in response to the `InterruptPipe` event the host will call driver callbacks as necessary.
    ///    As soon as the host is done with the buffer, it will call `pipe_continue`:
    ///    - if the `direction` is `In`: the host bus can now re-use the buffer, and wait for the next transfer from the device
    ///    - if the `direction` is `Out`: the driver has placed data into the buffer, and the host bus can transmit it
    /// 4. Finally, if the device disconnects (or a pipe is no longer needed), `release_interrupt_pipe` is called.
    ///
    /// The returned `InterruptPipe` contains two things:
    /// - `bus_ref`: this is a reference, used to associate `InterruptPipe` events as well as `pipe_continue` and `release_interrupt_pipe` calls to a particular pipe
    /// - `ptr`: pointer to the buffer used by this pipe. This is described below.
    ///
    /// ## Buffer pointer
    ///
    /// The buffer pointer returned for this InterruptPipe must:
    /// - point to a region with at least `size` bytes available
    /// - be valid at least until `release_interrupt_pipe` is called with the corresponding pipe ref
    ///
    /// Between any `Event::InterruptPipe` generated by the host bus, and the next corresponding call to `pipe_continue`,
    /// the host bus must not access or modify the buffer.
    ///
    /// For `In` pipes, the host will only read from this buffer, for `Out` pipes it will only write to it.
    ///
    fn create_interrupt_pipe(
        &mut self,
        device_address: DeviceAddress,
        endpoint_number: u8,
        direction: UsbDirection,
        size: u16,
        interval: u8,
    ) -> Option<InterruptPipe>;

    /// Release a pipe created with `create_interrupt_pipe`
    ///
    /// After a pipe is released, the `pipe_ref` as well as the buffer used by the pipe can be re-used.
    fn release_interrupt_pipe(&mut self, pipe_ref: u8);

    /// Signal that a pipe can continue transfers
    ///
    /// For an `In` pipe this is called after the driver(s) have consumed the data.
    /// For an `Out` pipe this is called after new data has been placed in the buffer .
    fn pipe_continue(&self, pipe_ref: u8);

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

/// Result from `create_interrupt_pipe`
pub struct InterruptPipe {
    /// Pointer to the buffer for this pipe
    ///
    /// See documentation for [`create_interrupt_pipe`](HostBus::create_interrupt_pipe) for details on how this is used.
    pub ptr: *mut u8,
    /// Reference for this pipe generated by the host bus
    ///
    /// This reference is used in three places:
    /// - in the [`Event::InterruptPipe`] event (generated by the host bus)
    /// - passed to [`pipe_continue`](HostBus::pipe_continue)
    /// - passed to [`release_interrupt_pipe`](HostBus::release_interrupt_pipe)
    pub bus_ref: u8,
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
    /// A start-of-frame packet has been sent
    ///
    /// This event must only be generated while start-of-frame interrupts are enabled.
    ///
    /// See [`HostBus::interrupt_on_sof`] for details.
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
