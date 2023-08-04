use super::Driver;
use core::num::NonZeroU8;
use crate::types::{DeviceAddress, ConnectionSpeed, DescriptorType, TransferType, SetupPacket};
use crate::bus::HostBus;
use crate::{UsbHost, PipeId};
use usb_device::{UsbDirection, control::{Recipient, RequestType}};

pub struct Kbd<const MAX_DEVICES: usize = 8> {
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
    input_report: [u8; 8],
}

impl PendingKbdDevice {
    fn supported_config(&self) -> Option<u8> {
        self.interface.and_then(|_| self.endpoint).and_then(|_| self.config)
    }
}


#[derive(Copy, Clone, defmt::Format)]
#[repr(packed)]
pub struct InputReport {
    modifier_status: ModifierStatus,
    _reserved: u8,
    keypress: [Option<NonZeroU8>; 6],
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
    pub fn left_ctrl(&self) -> bool {
        self.0 & 1 == 1
    }
    pub fn left_shift(&self) -> bool {
        (self.0 >> 1) & 1 == 1
    }
    pub fn left_alt(&self) -> bool {
        (self.0 >> 2) & 1 == 1
    }
    pub fn left_gui(&self) -> bool {
        (self.0 >> 3) & 1 == 1
    }
    pub fn right_ctrl(&self) -> bool {
        (self.0 >> 4) & 1 == 1
    }
    pub fn right_shift(&self) -> bool {
        (self.0 >> 5) & 1 == 1
    }
    pub fn right_alt(&self) -> bool {
        (self.0 >> 6) & 1 == 1
    }
    pub fn right_gui(&self) -> bool {
        (self.0 >> 7) & 1 == 1
    }
}

#[derive(Copy, Clone, defmt::Format)]
pub enum KbdEvent {
    DeviceAdded(DeviceAddress),
    DeviceRemoved(DeviceAddress),
    InputChanged(DeviceAddress, InputReport),
    ControlComplete(DeviceAddress),
}

impl<const MAX_DEVICES: usize> Kbd<MAX_DEVICES> {
    pub fn new() -> Self {
        Self { devices: [None; MAX_DEVICES], event: None }
    }

    /// Returns the last keyboard event that occurred (if any) and clears it.
    ///
    /// An event consists of a device address and an input report (8 bytes).
    pub fn take_event(&mut self) -> Option<KbdEvent> {
        self.event.take()
    }

    pub fn set_idle<B: HostBus>(&mut self, dev_addr: DeviceAddress, value: u16, host: &mut UsbHost<B>) {
        if let Some(device) = self.find_configured_device(dev_addr) {
            host.control_out(
                Some(dev_addr),
                Some(device.control_pipe),
                SetupPacket::new(
                    UsbDirection::Out,
                    RequestType::Class,
                    Recipient::Interface,
                    0x0a, // SetIdle
                    value,
                    device.interface as u16,
                    0,
                ),
                &[],
            );
        }
    }

    fn find_device_slot(&mut self, device_address: DeviceAddress) -> Option<&mut Option<KbdDevice>> {
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

    fn find_pending_device(&mut self, device_address: DeviceAddress) -> Option<&mut PendingKbdDevice> {
        match self.find_device(device_address) {
            Some(KbdDevice { inner: KbdDeviceInner::Pending(pending_device), .. }) => Some(pending_device),
            _ => None,
        }
    }

    fn find_configured_device(&mut self, device_address: DeviceAddress) -> Option<&mut ConfiguredKbdDevice> {
        match self.find_device(device_address) {
            Some(KbdDevice { inner: KbdDeviceInner::Configured(device), .. }) => Some(device),
            _ => None,
        }
    }

    fn remove_device(&mut self, device_address: DeviceAddress) {
        if let Some(slot) = self.find_device_slot(device_address) {
            slot.take();
        }
    }
}

impl<B: HostBus> Driver<B> for Kbd {
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
        defmt::info!("DRIVER DETACH {}", device_address);
        if let Some(slot) = self.find_device_slot(device_address) {
            defmt::info!("A");
            if let Some(KbdDevice { inner: KbdDeviceInner::Configured(_), .. }) = slot.take() {
                defmt::info!("B");
                self.event = Some(KbdEvent::DeviceRemoved(device_address));
            }
        }
    }

    fn descriptor(&mut self, device_address: DeviceAddress, descriptor_type: u8, data: &[u8]) {
        defmt::info!("Got desc {}, {}, {} bytes", device_address, descriptor_type, data.len());
        if let Some(device) = self.find_pending_device(device_address) {
            if descriptor_type == DescriptorType::Configuration as u8 {
                if device.interface.is_none() { // we only care about new configurations if we haven't already found an interface that we can handle
                    if let Ok((_, config)) = crate::descriptor::parse::configuration_descriptor(data) {
                        // keep track of the config value. If we encounter an interface descriptor within this configuration that
                        // we can handle, this will remain the final value.
                        // Otherwise the next config descriptor will overwrite it.
                        device.config = Some(config.value);
                    }
                }
            } else if descriptor_type == DescriptorType::Interface as u8 {
                if let Ok((_, interface)) = crate::descriptor::parse::interface_descriptor(data) {
                    if interface.interface_class == 0x03 && // HID
                        interface.interface_sub_class == 0x01 && // boot interface
                        interface.interface_protocol  == 0x01 { // keyboard
                            defmt::info!("Matching interface!");
                            device.interface = Some(interface.interface_number);
                        }
                }
            } else if descriptor_type == DescriptorType::Endpoint as u8 {
                if device.interface.is_some() && device.endpoint.is_none() {
                    if let Ok((_, endpoint)) = crate::descriptor::parse::endpoint_descriptor(data) {
                        if endpoint.address.direction() == UsbDirection::In && endpoint.attributes.transfer_type() == TransferType::Interrupt {
                            defmt::info!("Matching endpoint!");
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
        let config = self.find_pending_device(device_address).and_then(|device| device.supported_config());

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
                    // either a different configuration was selected, or we haven't found a matching interface
                    None
                } else {
                    let interface = device.interface.unwrap();
                    let control_pipe = host.create_control_pipe(device_address);
                    let interrupt_pipe = host.create_interrupt_pipe(
                        device_address,
                        device.endpoint.unwrap(),
                        UsbDirection::In,
                        8,
                        device.interval.unwrap(),
                    );
                    self.event = Some(KbdEvent::DeviceAdded(device_address));
                    match (control_pipe, interrupt_pipe) {
                        (Some(control_pipe), Some(interrupt_pipe)) => Some(ConfiguredKbdDevice {
                            interface,
                            control_pipe,
                            interrupt_pipe,
                            input_report: [0; 8],
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
            self.find_device_slot(device_address).unwrap().replace(KbdDevice {
                device_address,
                inner: KbdDeviceInner::Configured(configured_device),
            });
        } else {
            self.remove_device(device_address);
        }
    }

    fn completed_control(&mut self, dev_addr: DeviceAddress, _pipe_id: PipeId, _data: Option<&[u8]>) {
        self.event = Some(KbdEvent::ControlComplete(dev_addr));
    }

    fn completed_in(&mut self, device_address: DeviceAddress, pipe: PipeId, data: &[u8]) {
        if let Some(device) = self.find_configured_device(device_address) {
            if pipe == device.interrupt_pipe {
                let input_report: &InputReport = data.try_into().unwrap();
                self.event = Some(KbdEvent::InputChanged(device_address, *input_report));
            }
        }
    }

    fn completed_out(&mut self, _device_address: DeviceAddress, _pipe_id: PipeId, _data: &mut [u8]) {
        // ignored, since there are no OUT pipes in use.
    }
}
