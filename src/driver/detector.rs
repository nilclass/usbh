//! Helpers for detecting USB devices from drivers
//!

use crate::descriptor;
use crate::types::DeviceAddress;
use defmt::debug;

#[derive(Default)]
pub struct SimpleDetector<
    const CLASS_CODE: u8,
    const SUB_CLASS_CODE: u8,
    const EP_DIRECTION: u8,
    const EP_TYPE: u8,
    > {
    dev_addr: Option<DeviceAddress>,
    config: Option<u8>,
    interface: Option<u8>,
    endpoint: Option<(u8, u16, u8)>,
}

impl<
        const CLASS_CODE: u8,
    const SUB_CLASS_CODE: u8,
    const EP_DIRECTION: u8,
    const EP_TYPE: u8,
    > SimpleDetector<CLASS_CODE, SUB_CLASS_CODE, EP_DIRECTION, EP_TYPE> {

    fn reset(&mut self, dev_addr: Option<DeviceAddress>) {
        self.dev_addr = dev_addr;
        self.config = None;
        self.interface = None;
        self.endpoint = None;
    }

    pub fn attached(&mut self, dev_addr: DeviceAddress) {
        assert!(self.dev_addr == None);
        self.reset(Some(dev_addr));
    }

    pub fn detached(&mut self, _dev_addr: DeviceAddress) {
        self.reset(None);
    }

    pub fn descriptor(&mut self, dev_addr: DeviceAddress, descriptor_type: u8, data: &[u8]) {
        assert!(self.dev_addr == Some(dev_addr));
        match descriptor_type {
            descriptor::TYPE_CONFIGURATION => {
                debug!("check config");
                if self.endpoint.is_none() {
                    if let Ok((_, config)) = descriptor::parse::configuration_descriptor(data) {
                        self.config = Some(config.value);
                    }
                }
            }
            descriptor::TYPE_INTERFACE => {
                debug!("check iface");
                if let Ok((_, interface)) = descriptor::parse::interface_descriptor(data) {
                    if interface.interface_class == CLASS_CODE && interface.interface_sub_class == SUB_CLASS_CODE {
                        self.interface = Some(interface.interface_number);
                    }
                }
            }
            descriptor::TYPE_ENDPOINT => {
                debug!("check ep");
                if self.interface.is_some() {
                    if let Ok((_, endpoint)) = descriptor::parse::endpoint_descriptor(data) {
                        if endpoint.address.direction() as u8 == EP_DIRECTION && endpoint.attributes.transfer_type() as u8 == EP_TYPE {
                            self.endpoint = Some((endpoint.address.number(), endpoint.max_packet_size, endpoint.interval));
                        }
                    }
                }
            }
            _ => {
                // TODO
            }
        }
        debug!("{}, {}, {}, {}", self.dev_addr, self.config, self.interface, self.endpoint);
    }

    pub fn configure(&mut self, dev_addr: DeviceAddress) -> Option<u8> {
        assert!(self.dev_addr == Some(dev_addr));
        self.endpoint
            .and_then(|_| self.interface)
            .and_then(|_| self.config)
    }

    pub fn configured(&mut self, dev_addr: DeviceAddress, value: u8) -> Option<(u8, (u8, u16, u8))> {
        assert!(self.dev_addr == Some(dev_addr));
        let result = match self {
            Self { config: Some(config), interface: Some(interface), endpoint: Some(endpoint), .. } if *config == value => Some((*interface, *endpoint)),
            _ => None,
        };
        self.reset(None);
        result
    }
}
