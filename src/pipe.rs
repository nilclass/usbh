use usb_device::{UsbDirection, control::{Recipient, RequestType}};
use crate::{UsbHost, WouldBlock};
use crate::bus::HostBus;
use crate::types::{DeviceAddress, SetupPacket};

pub struct ControlPipe<B> {
    device_address: DeviceAddress,
    host: UsbHost<B>,
    
}

impl<B: HostBus> ControlPipe<B> {
    pub fn transfer_in(&mut self, request_type: RequestType, recipient: Recipient, request: u8, value: u16, index: u16, length: u16) -> Result<(), WouldBlock> {
        self.host.control_in(Some(self.device_address), SetupPacket::new(
            UsbDirection::In,
            request_type,
            recipient,
            request,
            value,
            index,
            length,
        ), length)
    }

    pub fn transfer_out(&mut self, request_type: RequestType, recipient: Recipient, request: u8, value: u16, index: u16, data: &[u8]) -> Result<(), WouldBlock> {
        self.host.control_out(Some(self.device_address), SetupPacket::new(
            UsbDirection::Out,
            request_type,
            recipient,
            request,
            value,
            index,
            data.len() as u16,
        ), data)
    }

    pub fn complete_in(&mut self) -> Option<&[u8]> {
    }
}
