use super::Driver;
use crate::bus::HostBus;
use crate::descriptor;
use crate::types::{ConnectionSpeed, DeviceAddress, SetupPacket, TransferType};
use crate::{ControlError, PipeId, UsbHost};
use core::num::NonZeroU8;
use usb_device::{
    control::{Recipient, RequestType},
    UsbDirection,
};

/// Driver for boot keyboards
///
/// By default, up to 8 connected keyboards can be handled. Events are reported for
/// each device separately.
///
/// To increase (or decrease) the number of devices that can be handled, adjust the `MAX_DEVICES` parameter.
///
/// Note: the number of devices that can be handled also depends on [`UsbHost`] which limits the number of pipes that can be created.
///   Each connected keyboard requires two pipes: a control pipe and an interrupt pipe.
pub struct KbdDriver<const MAX_DEVICES: usize = 8> {
    devices: [Option<KbdDevice>; MAX_DEVICES],
    event: Option<KbdEvent>,
}

#[derive(Copy, Clone)]
struct KbdDevice {
    device_address: DeviceAddress,
    inner: KbdDeviceInner,
}

#[derive(Copy, Clone)]
enum KbdDeviceInner {
    Pending(PendingKbdDevice),
    Configured(ConfiguredKbdDevice),
}

impl KbdDeviceInner {
    fn pending() -> Self {
        KbdDeviceInner::Pending(PendingKbdDevice {
            config: None,
            interface: None,
            endpoint: None,
            interval: None,
        })
    }
}

#[derive(Copy, Clone)]
struct PendingKbdDevice {
    config: Option<u8>,
    interface: Option<u8>,
    endpoint: Option<u8>,
    interval: Option<u8>,
}

#[derive(Copy, Clone)]
struct ConfiguredKbdDevice {
    interface: u8,
    control_pipe: PipeId,
    interrupt_pipe: PipeId,
    output_report: u8,
}

impl PendingKbdDevice {
    /// Returns the detected configuration value, if it is usable
    ///
    /// A configuration is ocnsidered usable, if it has:
    /// - an interface, with the correct class, subclass and protocol
    /// - an IN interrupt endpoint
    fn supported_config(&self) -> Option<u8> {
        self.interface
            .and_then(|_| self.endpoint)
            .and_then(|_| self.interval)
            .and_then(|_| self.config)
    }
}

/// Represents an input report, received from a keyboard
///
/// The input report describes which keys are currently pressed.
#[derive(Copy, Clone, defmt::Format)]
#[repr(packed)]
pub struct InputReport {
    /// Status of modifier keys
    pub modifier_status: ModifierStatus,
    _reserved: u8,

    pub keypress: [Option<NonZeroU8>; 6],
}

impl InputReport {
    pub fn pressed_keys(&self) -> impl Iterator<Item = u8> + '_ {
        self.keypress
            .iter()
            .filter_map(|opt| *opt)
            .map(|code| code.into())
    }
}

impl<'a> TryFrom<&'a [u8]> for &'a InputReport {
    type Error = ();

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        if value.len() == 8 && core::mem::size_of::<InputReport>() == 8 {
            // Safety: we have verified that the InputReport struct and the provided value have the expected size
            Ok(unsafe { &*(value as *const _ as *const InputReport) })
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Copy, Clone, defmt::Format)]
pub struct ModifierStatus(u8);

impl ModifierStatus {
    /// Is left `Ctrl` pressed?
    pub fn left_ctrl(&self) -> bool {
        self.0 & 1 == 1
    }

    /// Is left `Shift` pressed?
    pub fn left_shift(&self) -> bool {
        (self.0 >> 1) & 1 == 1
    }

    /// Is left `Alt` pressed?
    pub fn left_alt(&self) -> bool {
        (self.0 >> 2) & 1 == 1
    }

    /// Is left `Gui` pressed?
    ///
    /// The `Gui` button is also known as the `Super` or `Windows` key.
    pub fn left_gui(&self) -> bool {
        (self.0 >> 3) & 1 == 1
    }

    /// Is right `Ctrl` pressed?
    pub fn right_ctrl(&self) -> bool {
        (self.0 >> 4) & 1 == 1
    }

    /// Is right `Shift` pressed?
    pub fn right_shift(&self) -> bool {
        (self.0 >> 5) & 1 == 1
    }

    /// Is right `Alt` pressed?
    pub fn right_alt(&self) -> bool {
        (self.0 >> 6) & 1 == 1
    }

    /// Is right `Gui` pressed?
    ///
    /// The `Gui` button is also known as the `Super` or `Windows` key.
    pub fn right_gui(&self) -> bool {
        (self.0 >> 7) & 1 == 1
    }
}

/// Events related to attached keyboard(s)
#[derive(Copy, Clone, defmt::Format)]
pub enum KbdEvent {
    /// A new keyboard was detected & configured, with given device address
    DeviceAdded(DeviceAddress),

    /// A keyboard was removed
    DeviceRemoved(DeviceAddress),

    /// The input report changed for one of the devices.
    ///
    /// Use the [`InputReport`] object to find out more.
    InputChanged(DeviceAddress, InputReport),

    /// A control transfer has completed.
    ///
    /// Control transfers are initiated by the [`KbdDriver::set_idle`] and [`KbdDriver::set_led`] methods.
    ControlComplete(DeviceAddress),
}

/// Identifies the five LEDs that a boot keyboard can support
#[derive(Copy, Clone)]
#[repr(u8)]
pub enum KbdLed {
    NumLock = 0,
    CapsLock = 1,
    ScrollLock = 2,
    Compose = 3,
    Kana = 4,
}

/// Error type for interactions with the driver
#[derive(Copy, Clone)]
pub enum KbdError {
    /// Error initiating control transfer
    ControlError(ControlError),

    /// The given `DeviceAddress` is not known.
    ///
    /// This can happen if the device was removed meanwhile.
    UnknownDevice,
}

impl From<ControlError> for KbdError {
    fn from(e: ControlError) -> Self {
        KbdError::ControlError(e)
    }
}

impl<const MAX_DEVICES: usize> KbdDriver<MAX_DEVICES> {
    pub fn new() -> Self {
        Self {
            devices: [None; MAX_DEVICES],
            event: None,
        }
    }

    /// Returns the last keyboard event that occurred (if any) and clears it.
    ///
    /// This method should be called directly after calling `usb_host.poll(...)`.
    ///
    /// Otherwise events may be lost.
    ///
    /// For the meaning of events, please refer to the [`KbdEvent`] documentation.
    pub fn take_event(&mut self) -> Option<KbdEvent> {
        self.event.take()
    }

    /// Set interval for idle reports
    ///
    /// If an idle interval is set, the keyboard will send out the current input report (i.e. pressed keys)
    /// regularly, even when no change to the pressed keys occurs.
    ///
    /// The interval is expressed as a `duration` value, which is interpreted as a *multiple of 4 ms*.
    ///
    /// Setting the duration to `0` disables idle reports. If they are disabled, input reports are only
    /// received when a key is pressed or released.
    ///
    /// The USB HID specification recommends a default interval of 500ms for keyboards (duration value: 125).
    ///
    pub fn set_idle<B: HostBus>(
        &mut self,
        dev_addr: DeviceAddress,
        latency: u8,
        host: &mut UsbHost<B>,
    ) -> Result<(), KbdError> {
        if let Some(device) = self.find_configured_device(dev_addr) {
            host.control_out(
                Some(dev_addr),
                Some(device.control_pipe),
                SetupPacket::new(
                    UsbDirection::Out,
                    RequestType::Class,
                    Recipient::Interface,
                    0x0a, // SetIdle
                    (latency as u16) << 8,
                    device.interface as u16,
                    0,
                ),
                &[],
            )?;
            Ok(())
        } else {
            Err(KbdError::UnknownDevice)
        }
    }

    /// Set the given [`KbdLed`] to the specified state.
    ///
    /// The driver keeps track of the current output report (i.e. LED state basically) for each of the connected
    /// devices. Initially it is 0 (i.e. all LEDs are off).
    ///
    /// This method updates one of the bits in the output report (identified by [`KbdLed`]) and sents the
    /// updated report to the device.
    pub fn set_led<B: HostBus>(
        &mut self,
        dev_addr: DeviceAddress,
        led: KbdLed,
        on: bool,
        host: &mut UsbHost<B>,
    ) -> Result<(), KbdError> {
        if let Some(device) = self.find_configured_device(dev_addr) {
            if on {
                device.output_report |= 1 << (led as u8);
            } else {
                device.output_report &= !(1 << (led as u8));
            }
            host.control_out(
                Some(dev_addr),
                Some(device.control_pipe),
                SetupPacket::new(
                    UsbDirection::Out,
                    RequestType::Class,
                    Recipient::Interface,
                    0x09,   // SetReport,
                    2 << 8, // 2 means "output" report
                    0,
                    1,
                ),
                &[device.output_report],
            )?;
            Ok(())
        } else {
            Err(KbdError::UnknownDevice)
        }
    }

    fn find_device_slot(
        &mut self,
        device_address: DeviceAddress,
    ) -> Option<&mut Option<KbdDevice>> {
        self.devices.iter_mut().find(|dev| {
            if let Some(dev) = dev {
                dev.device_address == device_address
            } else {
                false
            }
        })
    }

    fn find_device(&mut self, device_address: DeviceAddress) -> Option<&mut KbdDevice> {
        if let Some(Some(device)) = self.find_device_slot(device_address) {
            Some(device)
        } else {
            None
        }
    }

    fn find_pending_device(
        &mut self,
        device_address: DeviceAddress,
    ) -> Option<&mut PendingKbdDevice> {
        match self.find_device(device_address) {
            Some(KbdDevice {
                inner: KbdDeviceInner::Pending(pending_device),
                ..
            }) => Some(pending_device),
            _ => None,
        }
    }

    fn find_configured_device(
        &mut self,
        device_address: DeviceAddress,
    ) -> Option<&mut ConfiguredKbdDevice> {
        match self.find_device(device_address) {
            Some(KbdDevice {
                inner: KbdDeviceInner::Configured(device),
                ..
            }) => Some(device),
            _ => None,
        }
    }

    fn remove_device(&mut self, device_address: DeviceAddress) {
        if let Some(slot) = self.find_device_slot(device_address) {
            slot.take();
        }
    }
}

impl<B: HostBus> Driver<B> for KbdDriver {
    fn attached(&mut self, device_address: DeviceAddress, _connection_speed: ConnectionSpeed) {
        if let Some(slot) = self.devices.iter_mut().find(|dev| dev.is_none()) {
            slot.replace(KbdDevice {
                device_address,
                inner: KbdDeviceInner::pending(),
            });
        } else {
            // maximum number of devices reached.
        }
    }

    fn detached(&mut self, device_address: DeviceAddress) {
        if let Some(slot) = self.find_device_slot(device_address) {
            if let Some(KbdDevice {
                inner: KbdDeviceInner::Configured(_),
                ..
            }) = slot.take()
            {
                self.event = Some(KbdEvent::DeviceRemoved(device_address));
            }
        }
    }

    fn descriptor(&mut self, device_address: DeviceAddress, descriptor_type: u8, data: &[u8]) {
        if let Some(device) = self.find_pending_device(device_address) {
            if descriptor_type == descriptor::TYPE_CONFIGURATION as u8 {
                if device.interface.is_none() {
                    // we only care about new configurations if we haven't already found an interface that we can handle
                    if let Ok((_, config)) = descriptor::parse::configuration_descriptor(data) {
                        // keep track of the config value. If we encounter an interface descriptor within this configuration that
                        // we can handle, this will remain the final value.
                        // Otherwise the next config descriptor will overwrite it.
                        device.config = Some(config.value);
                    }
                }
            } else if descriptor_type == descriptor::TYPE_INTERFACE {
                if let Ok((_, interface)) = descriptor::parse::interface_descriptor(data) {
                    if interface.interface_class == 0x03 && // HID
                        interface.interface_sub_class == 0x01 && // boot interface
                        interface.interface_protocol  == 0x01
                    {
                        // keyboard
                        device.interface = Some(interface.interface_number);
                    }
                }
            } else if descriptor_type == descriptor::TYPE_ENDPOINT {
                if device.interface.is_some() && device.endpoint.is_none() {
                    if let Ok((_, endpoint)) = descriptor::parse::endpoint_descriptor(data) {
                        if endpoint.address.direction() == UsbDirection::In
                            && endpoint.attributes.transfer_type() == TransferType::Interrupt
                        {
                            device.endpoint = Some(endpoint.address.number());
                            device.interval = Some(endpoint.interval);
                        }
                    }
                }
            }
        }
    }

    fn configure(&mut self, device_address: DeviceAddress) -> Option<u8> {
        // We choose a configuration only if we found an interface that we can handle
        let config = self
            .find_pending_device(device_address)
            .and_then(|device| device.supported_config());

        if config.is_none() {
            // clean up this device. We cannot handle it.
            self.remove_device(device_address);
        }

        config
    }

    fn configured(&mut self, device_address: DeviceAddress, value: u8, host: &mut UsbHost<B>) {
        let configured_device = if let Some(device) = self.find_pending_device(device_address) {
            if let Some(config) = device.supported_config() {
                if value != config {
                    // a different configuration was selected for this device. We can't handle it (probably).
                    None
                } else {
                    // Unwrap safety: supported_config() verifies there is a value
                    let interface = device.interface.unwrap();
                    let control_pipe = host.create_control_pipe(device_address);
                    let interrupt_pipe = host.create_interrupt_pipe(
                        device_address,
                        // Unwrap safety: supported_config() verifies there is a value
                        device.endpoint.unwrap(),
                        UsbDirection::In,
                        8,
                        // Unwrap safety: supported_config() verifies there is a value
                        device.interval.unwrap(),
                    );
                    self.event = Some(KbdEvent::DeviceAdded(device_address));
                    match (control_pipe, interrupt_pipe) {
                        (Some(control_pipe), Some(interrupt_pipe)) => Some(ConfiguredKbdDevice {
                            interface,
                            control_pipe,
                            interrupt_pipe,
                            output_report: 0,
                        }),
                        _ => None,
                    }
                }
            } else {
                // no supported configuration was found for the device
                None
            }
        } else {
            // we don't know this device (max devices reached, or already removed)
            None
        };

        if let Some(configured_device) = configured_device {
            // Unwrap safety: if `find_pending_device` above succeeded, then `find_device_slot` will succeed here as well
            self.find_device_slot(device_address)
                .unwrap()
                .replace(KbdDevice {
                    device_address,
                    inner: KbdDeviceInner::Configured(configured_device),
                });
        } else {
            self.remove_device(device_address);
        }
    }

    fn completed_control(
        &mut self,
        dev_addr: DeviceAddress,
        _pipe_id: PipeId,
        _data: Option<&[u8]>,
    ) {
        self.event = Some(KbdEvent::ControlComplete(dev_addr));
    }

    fn completed_in(&mut self, device_address: DeviceAddress, pipe: PipeId, data: &[u8]) {
        if let Some(device) = self.find_configured_device(device_address) {
            if pipe == device.interrupt_pipe {
                let converted: Result<&InputReport, _> = data.try_into();
                if let Ok(input_report) = converted {
                    self.event = Some(KbdEvent::InputChanged(device_address, *input_report));
                }
            }
        }
    }

    fn completed_out(
        &mut self,
        _device_address: DeviceAddress,
        _pipe_id: PipeId,
        _data: &mut [u8],
    ) {
        // ignored, since there are no OUT pipes in use.
    }
}
