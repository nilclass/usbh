use super::{
    Driver,
    detector::SimpleDetector,
};
use crate::{UsbHost, PipeId, ControlError};
use crate::bus::HostBus;
use crate::types::{ConnectionSpeed, DeviceAddress, TransferType, SetupPacket};
use usb_device::control::Request;
use usb_device::{UsbDirection, control::{Recipient, RequestType}};
use defmt::{error, debug, info, Format, bitflags};

#[derive(Copy, Clone)]
struct HubDevice {
    dev_addr: DeviceAddress,
    interface: u8,
    control_pipe: PipeId,
    interrupt_pipe: PipeId,
    control_state: ControlState,
}

#[derive(Copy, Clone, Format, PartialEq)]
enum ControlState {
    Idle,
    GetDescriptor,
    HubStatus,
    PortStatus(u8),
    SetPortFeature(u8, PortFeature),
    ClearPortFeature(u8, PortFeature),
}

#[derive(Copy, Clone, Format)]
pub struct HubDescriptor {
    pub port_count: u8,
    pub characteristics: Characteristics,
    pub power_on_to_good: u8,
    pub control_current: u8,
    pub device_removable: DeviceRemovable,
}

#[derive(Copy, Clone, Format)]
pub struct Characteristics(u16);

#[derive(Copy, Clone, Format)]
pub struct DeviceRemovable(u8);

fn parse_hub_descriptor(data: &[u8]) -> Option<HubDescriptor> {
    if data.len() < 8 {
        // too short
        None
    } else if data[1] != 0x29 {
        // not a hub descriptor
        None
    } else {
        Some(HubDescriptor {
            port_count: data[2],
            characteristics: Characteristics(((data[4] as u16) << 8) | (data[3] as u16)),
            power_on_to_good: data[5],
            control_current: data[6],
            device_removable: DeviceRemovable(data[7]),
        })
    }
}

fn parse_port_status(data: &[u8]) -> Option<PortStatus> {
    if data.len() != 4 {
        // invalid length
        None
    } else {
        Some(PortStatus {
            bits: (data[0] as u32) | ((data[1] as u32) << 8) | ((data[2] as u32) << 16) | ((data[3] as u32) << 24),
        })
    }
}

fn parse_hub_status(data: &[u8]) -> Option<HubStatus> {
    if data.len() != 4 {
        // invalid length
        None
    } else {
        Some(HubStatus(
            (data[0] as u16) | ((data[1] as u16) << 8),
            (data[2] as u16) | ((data[3] as u16) << 8),
        ))
    }
}

#[derive(Copy, Clone, Format, PartialEq)]
#[repr(u8)]
pub enum PortFeature {
    Connection = 0,
    Enable = 1,
    Suspend = 2,
    OverCurrent = 3,
    Reset = 4,
    Power = 8,
    LowSpeed = 9,
    CConnection = 16,
    CEnable = 17,
    CSuspend = 18,
    COverCurrent = 19,
    CReset = 20,
}

#[derive(Copy, Clone, Format)]
pub enum HubEvent {
    HubAdded(DeviceAddress),
    HubRemoved(DeviceAddress),
    Stall(DeviceAddress),
    HubDescriptor(DeviceAddress, HubDescriptor),
    HubStatus(DeviceAddress, HubStatus),
    PortStatus(DeviceAddress, u8, PortStatus),
    PortFeatureSet(DeviceAddress, u8, PortFeature),
    PortFeatureClear(DeviceAddress, u8, PortFeature),
    HubStatusChange(DeviceAddress),
    PortStatusChange(DeviceAddress, u8),
}

bitflags! {
    pub struct PortStatus: u32 {
        const CONNECTION = 1 << 0;
        const ENABLE = 1 << 1;
        const SUSPEND = 1 << 2;
        const OVER_CURRENT = 1 << 3;
        const RESET = 1 << 4;
        const POWER = 1 << 8;
        const LOW_SPEED = 1 << 9;
        const C_CONNECTION = 1 << 16;
        const C_ENABLE = 1 << 17;
        const C_SUSPEND = 1 << 18;
        const C_OVER_CURRENT = 1 << 19;
        const C_RESET = 1 << 20;
    }
}

#[derive(Copy, Clone, Format)]
pub struct HubStatus(u16, u16);

/// Error type for interactions with the driver
#[derive(Copy, Clone)]
pub enum HubError {
    /// Error initiating control transfer
    ControlError(ControlError),

    /// The given `DeviceAddress` is not known.
    ///
    /// This can happen if the device was removed meanwhile.
    UnknownDevice,
}

impl From<ControlError> for HubError {
    fn from(e: ControlError) -> Self {
        HubError::ControlError(e)
    }
}

/// A [`Driver`] which logs various events
pub struct HubDriver<const MAX_HUBS: usize = 4> {
    devices: [Option<HubDevice>; MAX_HUBS],
    detector: SimpleDetector<0x09, 0x00, { UsbDirection::In as u8 }, { TransferType::Interrupt as u8 }>,
    event: Option<HubEvent>,
}

impl<const MAX_HUBS: usize> HubDriver<MAX_HUBS> {
    pub fn new() -> Self {
        Self {
            devices: [None; MAX_HUBS],
            detector: SimpleDetector::default(),
            event: None,
        }
    }

    pub fn take_event(&mut self) -> Option<HubEvent> {
        self.event.take()
    }

    pub fn get_hub_descriptor<B: HostBus>(&mut self, dev_addr: DeviceAddress, host: &mut UsbHost<B>) -> Result<(), HubError> {
        if let Some(device) = self.find_device(dev_addr) {
            host.control_in(
                Some(dev_addr),
                Some(device.control_pipe),
                SetupPacket::new(
                    UsbDirection::In,
                    RequestType::Class,
                    Recipient::Device,
                    Request::GET_DESCRIPTOR,
                    0x29 << 8, // Hub
                    0,
                    8
                ),
            )?;
            device.control_state = ControlState::GetDescriptor;
            Ok(())
        } else {
            Err(HubError::UnknownDevice)
        }
    }

    pub fn get_hub_status<B: HostBus>(&mut self, dev_addr: DeviceAddress, host: &mut UsbHost<B>) -> Result<(), HubError> {
        if let Some(device) = self.find_device(dev_addr) {
            host.control_in(
                Some(dev_addr),
                Some(device.control_pipe),
                SetupPacket::new(
                    UsbDirection::In,
                    RequestType::Class,
                    Recipient::Device,
                    Request::GET_STATUS,
                    0,
                    0,
                    4,
                ),
            )?;
            device.control_state = ControlState::HubStatus;
            Ok(())
        } else {
            Err(HubError::UnknownDevice)
        }
    }

    pub fn get_port_status<B: HostBus>(&mut self, dev_addr: DeviceAddress, port: u8, host: &mut UsbHost<B>) -> Result<(), HubError> {
        if let Some(device) = self.find_device(dev_addr) {
            host.control_in(
                Some(dev_addr),
                Some(device.control_pipe),
                SetupPacket::new(
                    UsbDirection::In,
                    RequestType::Class,
                    Recipient::Other,
                    Request::GET_STATUS,
                    0,
                    port as u16,
                    4,
                ),
            )?;
            device.control_state = ControlState::PortStatus(port);
            Ok(())
        } else {
            Err(HubError::UnknownDevice)
        }
    }

    pub fn set_port_feature<B: HostBus>(&mut self, dev_addr: DeviceAddress, port: u8, feature: PortFeature, host: &mut UsbHost<B>) -> Result<(), HubError> {
        if let Some(device) = self.find_device(dev_addr) {
            host.control_out(
                Some(dev_addr), Some(device.control_pipe),
                SetupPacket::new(UsbDirection::Out, RequestType::Class, Recipient::Other, Request::SET_FEATURE, feature as u16, port as u16, 0),
                &[],
            )?;
            device.control_state = ControlState::SetPortFeature(port, feature);
            Ok(())
        } else {
            Err(HubError::UnknownDevice)
        }
    }

    pub fn clear_port_feature<B: HostBus>(&mut self, dev_addr: DeviceAddress, port: u8, feature: PortFeature, host: &mut UsbHost<B>) -> Result<(), HubError> {
        if let Some(device) = self.find_device(dev_addr) {
            host.control_out(
                Some(dev_addr), Some(device.control_pipe),
                SetupPacket::new(UsbDirection::Out, RequestType::Class, Recipient::Other, Request::CLEAR_FEATURE, feature as u16, port as u16, 0),
                &[],
            )?;
            device.control_state = ControlState::ClearPortFeature(port, feature);
            Ok(())
        } else {
            Err(HubError::UnknownDevice)
        }
    }

    fn find_device(&mut self, dev_addr: DeviceAddress) -> Option<&mut HubDevice> {
        self.devices.iter_mut().filter_map(|d| d.as_mut()).find(|d| d.dev_addr == dev_addr)
    }
}

impl<B: HostBus, const MAX_HUBS: usize> Driver<B> for HubDriver<MAX_HUBS> {
    fn attached(
        &mut self,
        dev_addr: DeviceAddress,
        _connection_speed: ConnectionSpeed,
    ) {
        self.detector.attached(dev_addr);
    }

    fn detached(&mut self, dev_addr: DeviceAddress) {
        if let Some(slot) = self.devices.iter_mut().find(|d| d.is_some() && d.unwrap().dev_addr == dev_addr) {
            slot.take();
            self.event = Some(HubEvent::HubRemoved(dev_addr));            
        } else {
            self.detector.detached(dev_addr);
        }
    }

    fn descriptor(&mut self, dev_addr: DeviceAddress, descriptor_type: u8, data: &[u8]) {
        self.detector.descriptor(dev_addr, descriptor_type, data);
    }

    fn configure(&mut self, dev_addr: DeviceAddress) -> Option<u8> {
        self.detector.configure(dev_addr)
    }

    fn configured(
        &mut self,
        dev_addr: DeviceAddress,
        value: u8,
        host: &mut UsbHost<B>,
    ) {
        if let Some((interface, (endpoint, size, interval))) = self.detector.configured(dev_addr, value) {
            if let Some(slot) = self.devices.iter_mut().find(|d| d.is_none()) {
                match (
                    host.create_control_pipe(dev_addr),
                    host.create_interrupt_pipe(dev_addr, endpoint, UsbDirection::In, size, interval),
                ) {
                    (Some(control_pipe), None) => host.release_pipe(control_pipe),
                    (None, Some(interrupt_pipe)) => host.release_pipe(interrupt_pipe),
                    (Some(control_pipe), Some(interrupt_pipe)) => {
                        slot.replace(HubDevice {
                            dev_addr,
                            interface,
                            control_pipe,
                            interrupt_pipe,
                            control_state: ControlState::Idle,
                        });
                        self.event = Some(HubEvent::HubAdded(dev_addr));
                    },
                    (None, None) => {},
                }
            }
        }
    }

    fn completed_control(
        &mut self,
        dev_addr: DeviceAddress,
        pipe_id: crate::PipeId,
        data: Option<&[u8]>,
    ) {
        if let Some(device) = self.find_device(dev_addr) {
            if pipe_id == device.control_pipe {
                match device.control_state {
                    ControlState::Idle => {},
                    ControlState::GetDescriptor => {
                        if let Some(desc) = data.and_then(parse_hub_descriptor) {
                            device.control_state = ControlState::Idle;
                            self.event = Some(HubEvent::HubDescriptor(dev_addr, desc));
                        }
                    }
                    ControlState::HubStatus => {
                        if let Some(status) = data.and_then(parse_hub_status) {
                            device.control_state = ControlState::Idle;
                            self.event = Some(HubEvent::HubStatus(dev_addr, status));
                        }
                    }
                    ControlState::PortStatus(port) => {
                        if let Some(port_status) = data.and_then(parse_port_status) {
                            device.control_state = ControlState::Idle;
                            self.event = Some(HubEvent::PortStatus(dev_addr, port, port_status));
                        }
                    }
                    ControlState::SetPortFeature(port, feature) => {
                        device.control_state = ControlState::Idle;
                        self.event = Some(HubEvent::PortFeatureSet(dev_addr, port, feature));
                    }
                    ControlState::ClearPortFeature(port, feature) => {
                        device.control_state = ControlState::Idle;
                        self.event = Some(HubEvent::PortFeatureClear(dev_addr, port, feature));
                    }
                }
            }
        }
    }

    fn completed_in(
        &mut self,
        dev_addr: DeviceAddress,
        pipe_id: crate::PipeId,
        data: &[u8],
    ) {
        if let Some(device) = self.find_device(dev_addr) {
            if pipe_id == device.interrupt_pipe {
                let status = data[0];
                let mut bit = None;
                for i in 0..32 {
                    if (status >> i) & 1 == 1 {
                        bit = Some(i);
                        break;
                    }
                }

                if let Some(bit) = bit {
                    if bit == 0 {
                        self.event = Some(HubEvent::HubStatusChange(dev_addr));
                    } else {
                        self.event = Some(HubEvent::PortStatusChange(dev_addr, bit));
                    }
                }
            };
        }
    }

    fn completed_out(
        &mut self,
        dev_addr: DeviceAddress,
        pipe_id: crate::PipeId,
        _data: &mut [u8],
    ) {
        todo!()
        // TODO
    }

    fn stall(
        &mut self,
        dev_addr: DeviceAddress,
    ) {
        if let Some(device) = self.find_device(dev_addr) {
            if device.control_state != ControlState::Idle {
                error!("Stall received, aborting control state {}", device.control_state);
            }
            self.event = Some(HubEvent::Stall(dev_addr));
        }
    }
}
