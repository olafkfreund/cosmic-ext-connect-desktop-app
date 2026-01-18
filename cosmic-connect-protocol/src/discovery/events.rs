//! Discovery Event System
//!
//! This module defines events emitted by the discovery service.

use crate::transport::{TransportAddress, TransportType};
use crate::DeviceInfo;
use std::net::SocketAddr;

/// Events emitted by the discovery service
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// A new device was discovered on the network
    DeviceDiscovered {
        /// Information about the discovered device
        info: DeviceInfo,
        /// Network address where the device was discovered (deprecated, use transport_address)
        #[deprecated(since = "0.2.0", note = "Use transport_address instead")]
        address: SocketAddr,
        /// Transport address (supports both TCP and Bluetooth)
        transport_address: TransportAddress,
        /// Transport type used for discovery
        transport_type: TransportType,
    },

    /// An existing device sent an updated identity packet
    DeviceUpdated {
        /// Updated device information
        info: DeviceInfo,
        /// Network address of the device (deprecated, use transport_address)
        #[deprecated(since = "0.2.0", note = "Use transport_address instead")]
        address: SocketAddr,
        /// Transport address (supports both TCP and Bluetooth)
        transport_address: TransportAddress,
        /// Transport type used for discovery
        transport_type: TransportType,
    },

    /// A device has timed out (not seen for configured duration)
    DeviceTimeout {
        /// ID of the device that timed out
        device_id: String,
    },

    /// Discovery service started successfully
    ServiceStarted {
        /// Port the discovery service is listening on
        port: u16,
    },

    /// Discovery service stopped
    ServiceStopped,

    /// An error occurred during discovery
    Error {
        /// Error message
        message: String,
    },
}

impl DiscoveryEvent {
    /// Create a device discovered event from TCP/IP discovery
    pub fn tcp_discovered(info: DeviceInfo, address: SocketAddr) -> Self {
        DiscoveryEvent::DeviceDiscovered {
            info,
            #[allow(deprecated)]
            address,
            transport_address: TransportAddress::Tcp(address),
            transport_type: TransportType::Tcp,
        }
    }

    /// Create a device discovered event from Bluetooth discovery
    pub fn bluetooth_discovered(info: DeviceInfo, bt_address: String) -> Self {
        DiscoveryEvent::DeviceDiscovered {
            info,
            #[allow(deprecated)]
            address: "0.0.0.0:0".parse().unwrap(), // Placeholder for backward compatibility
            transport_address: TransportAddress::Bluetooth {
                address: bt_address,
                service_uuid: Some(crate::transport::CCONNECT_SERVICE_UUID),
            },
            transport_type: TransportType::Bluetooth,
        }
    }

    /// Create a device updated event from TCP/IP discovery
    pub fn tcp_updated(info: DeviceInfo, address: SocketAddr) -> Self {
        DiscoveryEvent::DeviceUpdated {
            info,
            #[allow(deprecated)]
            address,
            transport_address: TransportAddress::Tcp(address),
            transport_type: TransportType::Tcp,
        }
    }

    /// Create a device updated event from Bluetooth discovery
    pub fn bluetooth_updated(info: DeviceInfo, bt_address: String) -> Self {
        DiscoveryEvent::DeviceUpdated {
            info,
            #[allow(deprecated)]
            address: "0.0.0.0:0".parse().unwrap(), // Placeholder for backward compatibility
            transport_address: TransportAddress::Bluetooth {
                address: bt_address,
                service_uuid: Some(crate::transport::CCONNECT_SERVICE_UUID),
            },
            transport_type: TransportType::Bluetooth,
        }
    }

    /// Check if this is a device discovered event
    pub fn is_device_discovered(&self) -> bool {
        matches!(self, DiscoveryEvent::DeviceDiscovered { .. })
    }

    /// Check if this is a device updated event
    pub fn is_device_updated(&self) -> bool {
        matches!(self, DiscoveryEvent::DeviceUpdated { .. })
    }

    /// Check if this is a device timeout event
    pub fn is_device_timeout(&self) -> bool {
        matches!(self, DiscoveryEvent::DeviceTimeout { .. })
    }

    /// Get device ID if this event is device-related
    pub fn device_id(&self) -> Option<&str> {
        match self {
            DiscoveryEvent::DeviceDiscovered { info, .. } => Some(&info.device_id),
            DiscoveryEvent::DeviceUpdated { info, .. } => Some(&info.device_id),
            DiscoveryEvent::DeviceTimeout { device_id } => Some(device_id),
            _ => None,
        }
    }

    /// Get transport type if this is a device event
    pub fn transport_type(&self) -> Option<TransportType> {
        match self {
            DiscoveryEvent::DeviceDiscovered { transport_type, .. } => Some(*transport_type),
            DiscoveryEvent::DeviceUpdated { transport_type, .. } => Some(*transport_type),
            _ => None,
        }
    }

    /// Get transport address if this is a device event
    pub fn transport_address(&self) -> Option<&TransportAddress> {
        match self {
            DiscoveryEvent::DeviceDiscovered {
                transport_address, ..
            } => Some(transport_address),
            DiscoveryEvent::DeviceUpdated {
                transport_address, ..
            } => Some(transport_address),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceType;

    #[test]
    fn test_event_type_checking() {
        let info = DeviceInfo::new("Test", DeviceType::Desktop, 1816);
        let addr = "192.168.1.100:1816".parse().unwrap();

        let discovered = DiscoveryEvent::tcp_discovered(info.clone(), addr);
        assert!(discovered.is_device_discovered());
        assert!(!discovered.is_device_timeout());

        let timeout = DiscoveryEvent::DeviceTimeout {
            device_id: "test_id".to_string(),
        };
        assert!(timeout.is_device_timeout());
        assert!(!timeout.is_device_discovered());
    }

    #[test]
    fn test_device_id_extraction() {
        let info = DeviceInfo::with_id("test_123", "Test", DeviceType::Desktop, 1816);
        let addr = "192.168.1.100:1816".parse().unwrap();

        let discovered = DiscoveryEvent::tcp_discovered(info.clone(), addr);
        assert_eq!(discovered.device_id(), Some("test_123"));

        let timeout = DiscoveryEvent::DeviceTimeout {
            device_id: "timeout_id".to_string(),
        };
        assert_eq!(timeout.device_id(), Some("timeout_id"));

        let started = DiscoveryEvent::ServiceStarted { port: 1816 };
        assert_eq!(started.device_id(), None);
    }

    #[test]
    fn test_tcp_discovery_event() {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1816);
        let addr = "192.168.1.100:1816".parse().unwrap();

        let event = DiscoveryEvent::tcp_discovered(info.clone(), addr);

        assert!(event.is_device_discovered());
        assert_eq!(event.transport_type(), Some(TransportType::Tcp));

        match event.transport_address() {
            Some(TransportAddress::Tcp(a)) => assert_eq!(a, &addr),
            _ => panic!("Expected TCP transport address"),
        }
    }

    #[test]
    fn test_bluetooth_discovery_event() {
        let info = DeviceInfo::new("Test Phone", DeviceType::Phone, 1816);
        let bt_addr = "00:11:22:33:44:55".to_string();

        let event = DiscoveryEvent::bluetooth_discovered(info.clone(), bt_addr.clone());

        assert!(event.is_device_discovered());
        assert_eq!(event.transport_type(), Some(TransportType::Bluetooth));

        match event.transport_address() {
            Some(TransportAddress::Bluetooth {
                address,
                service_uuid,
            }) => {
                assert_eq!(address, &bt_addr);
                assert_eq!(*service_uuid, Some(crate::transport::CCONNECT_SERVICE_UUID));
            }
            _ => panic!("Expected Bluetooth transport address"),
        }
    }
}
