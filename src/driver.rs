use core::num::NonZeroU8;
use crate::types::{DeviceAddress, ConnectionSpeed, DescriptorType, TransferType, SetupPacket};
use crate::bus::HostBus;
use crate::{UsbHost, PipeId};
use usb_device::{UsbDirection, control::{Recipient, RequestType}};

// pub trait Driver<B: HostBus> {
//     /// New device was attached, and got assigned the given address.
//     ///
//     /// This is a good time to request some descriptors.
//     fn attached(&mut self, device_address: DeviceAddress, host: &mut UsbHost<B>);

//     /// The device with the given address was detached.
//     ///
//     /// Clean up any internal data related to the device here.
//     fn detached(&mut self, device_address: DeviceAddress);

//     fn transfer_in_complete(&mut self, device_address: DeviceAddress, length: usize, host: &mut UsbHost<B>);

//     fn transfer_out_complete(&mut self, device_address: DeviceAddress, host: &mut UsbHost<B>);

//     fn interrupt_in_complete(&mut self, device_address: DeviceAddress, length: usize, host: &mut UsbHost<B>);

//     fn interrupt_out_complete(&mut self, device_address: DeviceAddress, host: &mut UsbHost<B>);

//     fn pipe_event(&mut self, device_address: DeviceAddress, data: &[u8]);
// }

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
        if let Some(slot) = self.find_device_slot(device_address) {
            if let Some(KbdDevice { inner: KbdDeviceInner::Configured(_), .. }) = slot.take() {
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
