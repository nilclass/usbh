use super::Driver;
use crate::bus::HostBus;
use crate::descriptor;
use defmt::{bitflags, info};

/// A [`Driver`] which logs various events
pub struct LogDriver(EventMask);

bitflags! {
    /// Used to select which events are logged by the [`LogDriver`]
    ///
    /// Each of the flags corresponds to one of the methods in the [`Driver`] interface.
    pub struct EventMask: u8 {
        const ATTACHED = 1 << 0;
        const DETACHED = 1 << 1;
        const DESCRIPTOR = 1 << 2;
        const CONFIGURE = 1 << 3;
        const CONFIGURED = 1 << 4;
        const COMPLETED_CONTROL = 1 << 5;
        const COMPLETED_IN = 1 << 6;
        const COMPLETED_OUT = 1 << 7;
    }
}

impl LogDriver {
    pub fn new(event_mask: EventMask) -> Self {
        Self(event_mask)
    }
}

impl<B: HostBus> Driver<B> for LogDriver {
    fn attached(
        &mut self,
        dev_addr: crate::types::DeviceAddress,
        connection_speed: crate::types::ConnectionSpeed,
    ) {
        if self.0.contains(EventMask::ATTACHED) {
            info!(
                "[usbh LogDriver] New {}-speed device attached, with assigned address {}",
                connection_speed,
                u8::from(dev_addr)
            );
        }
    }

    fn detached(&mut self, dev_addr: crate::types::DeviceAddress) {
        if self.0.contains(EventMask::DETACHED) {
            info!(
                "[usbh LogDriver] Device {} was detached",
                u8::from(dev_addr)
            );
        }
    }

    fn descriptor(
        &mut self,
        dev_addr: crate::types::DeviceAddress,
        descriptor_type: u8,
        data: &[u8],
    ) {
        if self.0.contains(EventMask::DESCRIPTOR) {
            match descriptor_type {
                descriptor::TYPE_DEVICE => {
                    let descriptor = descriptor::parse::device_descriptor(data)
                        .map(|(_, desc)| desc)
                        .map_err(|_| "(parse failed)");
                    info!(
                        "[usbh LogDriver] Device {} sent device descriptor:\n  {:#X}",
                        u8::from(dev_addr),
                        descriptor,
                    )
                }
                descriptor::TYPE_CONFIGURATION => {
                    let descriptor = descriptor::parse::configuration_descriptor(data)
                        .map(|(_, desc)| desc)
                        .map_err(|_| "(parse failed)");
                    info!(
                        "[usbh LogDriver] Device {} sent configuration descriptor:\n  {:#X}",
                        u8::from(dev_addr),
                        descriptor,
                    )
                }
                descriptor::TYPE_STRING => {
                    info!(
                        "[usbh LogDriver] Device {} sent string descriptor:\n  {:#X}",
                        u8::from(dev_addr),
                        data,
                    )
                }
                descriptor::TYPE_INTERFACE => {
                    let descriptor = descriptor::parse::interface_descriptor(data)
                        .map(|(_, desc)| desc)
                        .map_err(|_| "(parse failed)");
                    info!(
                        "[usbh LogDriver] Device {} sent interface descriptor:\n  {:#X}",
                        u8::from(dev_addr),
                        descriptor,
                    )
                }
                descriptor::TYPE_ENDPOINT => {
                    let descriptor = descriptor::parse::endpoint_descriptor(data)
                        .map(|(_, desc)| desc)
                        .map_err(|_| "(parse failed)");
                    info!(
                        "[usbh LogDriver] Device {} sent endpoint descriptor:\n  {:#X}",
                        u8::from(dev_addr),
                        descriptor,
                    )
                }
                _ => {
                    info!(
                        "[usbh LogDriver] Device {} sent descriptor of type {:#X}: {}",
                        u8::from(dev_addr),
                        descriptor_type,
                        data,
                    )
                }
            }
        }
    }

    fn configure(&mut self, dev_addr: crate::types::DeviceAddress) -> Option<u8> {
        if self.0.contains(EventMask::CONFIGURE) {
            info!(
                "[usbh LogDriver] Device {} is looking for a configuration",
                u8::from(dev_addr)
            );
        }
        None
    }

    fn configured(
        &mut self,
        dev_addr: crate::types::DeviceAddress,
        value: u8,
        _host: &mut crate::UsbHost<B>,
    ) {
        if self.0.contains(EventMask::CONFIGURED) {
            info!(
                "[usbh LogDriver] Device {} was configured with configuration {}",
                u8::from(dev_addr),
                value
            );
        }
    }

    fn completed_control(
        &mut self,
        dev_addr: crate::types::DeviceAddress,
        pipe_id: crate::PipeId,
        data: Option<&[u8]>,
    ) {
        if self.0.contains(EventMask::COMPLETED_CONTROL) {
            info!(
                "[usbh LogDriver] Device {}: completed control {} transfer on pipe {}",
                u8::from(dev_addr),
                if data.is_some() { "IN" } else { "OUT" },
                pipe_id.0,
            );
        }
    }

    fn completed_in(
        &mut self,
        dev_addr: crate::types::DeviceAddress,
        pipe_id: crate::PipeId,
        _data: &[u8],
    ) {
        if self.0.contains(EventMask::COMPLETED_IN) {
            info!(
                "[usbh LogDriver] Device {}: completed IN transfer on pipe {}",
                u8::from(dev_addr),
                pipe_id.0,
            );
        }
    }

    fn completed_out(
        &mut self,
        dev_addr: crate::types::DeviceAddress,
        pipe_id: crate::PipeId,
        _data: &mut [u8],
    ) {
        if self.0.contains(EventMask::COMPLETED_OUT) {
            info!(
                "[usbh LogDriver] Device {}: completed OUT transfer on pipe {}",
                u8::from(dev_addr),
                pipe_id.0,
            );
        }
    }
}
