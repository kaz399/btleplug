// btleplug Source Code File
//
// Copyright 2020 Nonpolynomial Labs LLC. All rights reserved.
//
// Licensed under the BSD 3-Clause license. See LICENSE file in the project root
// for full license information.
//
// Some portions of this file are taken and/or modified from Rumble
// (https://github.com/mwylde/rumble), using a dual MIT/Apache License under the
// following copyright:
//
// Copyright (c) 2014 The Rust Project Developers

mod adapter_manager;
pub mod bleuuid;

use crate::{Error, Result};
pub use adapter_manager::AdapterManager;
use bitflags::bitflags;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "serde")]
use serde_cr as serde;
use std::sync::mpsc::Receiver;
use std::{
    collections::{BTreeSet, HashMap},
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
    str::FromStr,
};
use thiserror::Error;
use uuid::Uuid;

#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_cr")
)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AddressType {
    Random,
    Public,
}

impl Default for AddressType {
    fn default() -> Self {
        AddressType::Public
    }
}

impl AddressType {
    pub fn from_str(v: &str) -> Option<AddressType> {
        match v {
            "public" => Some(AddressType::Public),
            "random" => Some(AddressType::Random),
            _ => None,
        }
    }

    pub fn from_u8(v: u8) -> Option<AddressType> {
        match v {
            1 => Some(AddressType::Public),
            2 => Some(AddressType::Random),
            _ => None,
        }
    }

    pub fn num(&self) -> u8 {
        match *self {
            AddressType::Public => 1,
            AddressType::Random => 2,
        }
    }
}

/// Stores the 6 byte address used to identify Bluetooth devices.
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_cr")
)]
#[derive(Copy, Clone, Hash, Eq, PartialEq, Default)]
#[repr(C)]
pub struct BDAddr {
    pub address: [u8; 6usize],
}

impl Display for BDAddr {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let a = self.address;
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            a[5], a[4], a[3], a[2], a[1], a[0]
        )
    }
}

impl Debug for BDAddr {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        (self as &dyn Display).fmt(f)
    }
}

type ParseBDAddrResult<T> = std::result::Result<T, ParseBDAddrError>;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ParseBDAddrError {
    #[error("Bluetooth address has to be 6 bytes long")]
    IncorrectByteCount,
    #[error("Malformed integer in Bluetooth address")]
    InvalidInt,
}

impl From<ParseBDAddrError> for Error {
    fn from(e: ParseBDAddrError) -> Self {
        Error::Other(format!("ParseBDAddrError: {}", e))
    }
}

impl From<::uuid::Error> for Error {
    fn from(error: ::uuid::Error) -> Self {
        Error::Other(format!("Error parsing UUID: {}", error))
    }
}

impl FromStr for BDAddr {
    type Err = ParseBDAddrError;

    fn from_str(s: &str) -> ParseBDAddrResult<Self> {
        let bytes = s
            .split(':')
            .map(|part: &str| {
                u8::from_str_radix(part, 16).map_err(|_| ParseBDAddrError::InvalidInt)
            })
            .collect::<ParseBDAddrResult<Vec<u8>>>()?;

        if let Ok(mut address) = <[u8; 6]>::try_from(bytes.as_slice()) {
            address.reverse();
            Ok(BDAddr { address })
        } else {
            Err(ParseBDAddrError::IncorrectByteCount)
        }
    }
}

/// A notification sent from a peripheral due to a change in a value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueNotification {
    /// UUID of the characteristic that fired the notification.
    pub uuid: Uuid,
    /// The handle that has changed. Only valid on Linux, will be None on all
    /// other platforms.
    pub handle: Option<u16>,
    /// The new value of the handle.
    pub value: Vec<u8>,
}

pub type Callback<T> = Box<dyn Fn(Result<T>) + Send>;

pub type NotificationHandler = Box<dyn FnMut(ValueNotification) + Send>;

bitflags! {
    /// A set of properties that indicate what operations are supported by a Characteristic.
    pub struct CharPropFlags: u8 {
        const BROADCAST = 0x01;
        const READ = 0x02;
        const WRITE_WITHOUT_RESPONSE = 0x04;
        const WRITE = 0x08;
        const NOTIFY = 0x10;
        const INDICATE = 0x20;
        const AUTHENTICATED_SIGNED_WRITES = 0x40;
        const EXTENDED_PROPERTIES = 0x80;
    }
}

impl CharPropFlags {
    pub fn new() -> Self {
        Self { bits: 0 }
    }
}

/// A Bluetooth characteristic. Characteristics are the main way you will interact with other
/// bluetooth devices. Characteristics are identified by a UUID which may be standardized
/// (like 0x2803, which identifies a characteristic for reading heart rate measurements) but more
/// often are specific to a particular device. The standard set of characteristics can be found
/// [here](https://www.bluetooth.com/specifications/gatt/characteristics).
///
/// A characteristic may be interacted with in various ways depending on its properties. You may be
/// able to write to it, read from it, set its notify or indicate status, or send a command to it.
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Clone)]
pub struct Characteristic {
    /// The start of the handle range that contains this characteristic. Only
    /// valid on Linux, will be 0 on all other platforms.
    pub start_handle: u16,
    /// The end of the handle range that contains this characteristic. Only
    /// valid on Linux, will be 0 on all other platforms.
    pub end_handle: u16,
    /// The value handle of the characteristic. Only
    /// valid on Linux, will be 0 on all other platforms.
    pub value_handle: u16,
    /// The UUID for this characteristic. This uniquely identifies its behavior.
    pub uuid: Uuid,
    /// The set of properties for this characteristic, which indicate what functionality it
    /// supports. If you attempt an operation that is not supported by the characteristics (for
    /// example setting notify on one without the NOTIFY flag), that operation will fail.
    pub properties: CharPropFlags,
}

impl Display for Characteristic {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "uuid: {:?}, char properties: {:?}",
            self.uuid, self.properties
        )
    }
}

/// The properties of this peripheral, as determined by the advertising reports we've received for
/// it.
#[derive(Debug, Default, Clone)]
pub struct PeripheralProperties {
    /// The address of this peripheral
    pub address: BDAddr,
    /// The type of address (either random or public)
    pub address_type: AddressType,
    /// The local name. This is generally a human-readable string that identifies the type of device.
    pub local_name: Option<String>,
    /// The transmission power level for the device
    pub tx_power_level: Option<i8>,
    /// Advertisement data specific to the device manufacturer. The keys of this map are
    /// 'manufacturer IDs', while the values are arbitrary data.
    pub manufacturer_data: HashMap<u16, Vec<u8>>,
    /// Advertisement data specific to a service. The keys of this map are
    /// 'Service UUIDs', while the values are arbitrary data.
    pub service_data: HashMap<Uuid, Vec<u8>>,
    /// Advertised services for this device
    pub services: Vec<Uuid>,
    /// Number of times we've seen advertising reports for this device
    pub discovery_count: u32,
    /// True if we've discovered the device before
    pub has_scan_response: bool,
}

/// The type of write operation to use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteType {
    /// A write operation where the device is expected to respond with a confirmation or error. Also
    /// known as a request.
    WithResponse,
    /// A write-without-response, also known as a command.
    WithoutResponse,
}

/// Peripheral is the device that you would like to communicate with (the "server" of BLE). This
/// struct contains both the current state of the device (its properties, characteristics, etc.)
/// as well as functions for communication.
pub trait Peripheral: Send + Sync + Clone + Debug {
    /// Returns the address of the peripheral.
    fn address(&self) -> BDAddr;

    /// Returns the set of properties associated with the peripheral. These may be updated over time
    /// as additional advertising reports are received.
    fn properties(&self) -> PeripheralProperties;

    /// The set of characteristics we've discovered for this device. This will be empty until
    /// `discover_characteristics` is called.
    fn characteristics(&self) -> BTreeSet<Characteristic>;

    /// Returns true iff we are currently connected to the device.
    fn is_connected(&self) -> bool;

    /// Creates a connection to the device. This is a synchronous operation; if this method returns
    /// Ok there has been successful connection. Note that peripherals allow only one connection at
    /// a time. Operations that attempt to communicate with a device will fail until it is connected.
    fn connect(&self) -> Result<()>;

    /// Terminates a connection to the device. This is a synchronous operation.
    fn disconnect(&self) -> Result<()>;

    /// Discovers all characteristics for the device. This is a synchronous operation.
    fn discover_characteristics(&self) -> Result<Vec<Characteristic>>;

    /// Write some data to the characteristic. Returns an error if the write couldn't be send or (in
    /// the case of a write-with-response) if the device returns an error.
    fn write(
        &self,
        characteristic: &Characteristic,
        data: &[u8],
        write_type: WriteType,
    ) -> Result<()>;

    /// Sends a request (read) to the device. Synchronously returns either an error if the request
    /// was not accepted or the response from the device.
    fn read(&self, characteristic: &Characteristic) -> Result<Vec<u8>>;

    /// Sends a read-by-type request to device for the range of handles covered by the
    /// characteristic and for the specified declaration UUID. See
    /// [here](https://www.bluetooth.com/specifications/gatt/declarations) for valid UUIDs.
    /// Synchronously returns either an error or the device response.
    fn read_by_type(&self, characteristic: &Characteristic, uuid: Uuid) -> Result<Vec<u8>>;

    /// Enables either notify or indicate (depending on support) for the specified characteristic.
    /// This is a synchronous call.
    fn subscribe(&self, characteristic: &Characteristic) -> Result<()>;

    /// Disables either notify or indicate (depending on support) for the specified characteristic.
    /// This is a synchronous call.
    fn unsubscribe(&self, characteristic: &Characteristic) -> Result<()>;

    /// Registers a handler that will be called when value notification messages are received from
    /// the device. This method should only be used after a connection has been established. Note
    /// that the handler will be called in a common thread, so it should not block.
    fn on_notification(&self, handler: NotificationHandler);
}

#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_cr")
)]
#[derive(Debug, Clone)]
pub enum CentralEvent {
    DeviceDiscovered(BDAddr),
    DeviceLost(BDAddr),
    DeviceUpdated(BDAddr),
    DeviceConnected(BDAddr),
    DeviceDisconnected(BDAddr),
    /// Emitted when a Manufacturer Data advertisement has been received from a device
    ManufacturerDataAdvertisement {
        address: BDAddr,
        manufacturer_id: u16,
        data: Vec<u8>,
    },
    /// Emitted when a Service Data advertisement has been received from a device
    ServiceDataAdvertisement {
        address: BDAddr,
        service: Uuid,
        data: Vec<u8>,
    },
    /// Emitted when the advertised services for a device has been updated
    ServicesAdvertisement {
        address: BDAddr,
        services: Vec<Uuid>,
    },
}

/// Central is the "client" of BLE. It's able to scan for and establish connections to peripherals.
pub trait Central<P: Peripheral>: Send + Sync + Clone {
    /// Retreive the Event [Receiver] for the event channel. This channel
    /// receiver will receive notifications when events occur for this Central
    /// module. As this uses an std::channel which cannot be cloned, after the
    /// first call (which will contain Some<Receiver<CentralEvent>>), all
    /// subsequent calls will return None. See [`Event`](enum.CentralEvent.html)
    /// for the full set of events returned.
    fn event_receiver(&self) -> Option<Receiver<CentralEvent>>;

    /// Starts a scan for BLE devices. This scan will generally continue until explicitly stopped,
    /// although this may depend on your bluetooth adapter. Discovered devices will be announced
    /// to subscribers of `on_event` and will be available via `peripherals()`.
    fn start_scan(&self) -> Result<()>;

    /// Control whether to use active or passive scan mode to find BLE devices. Active mode scan
    /// notifies advertises about the scan, whereas passive scan only receives data from the
    /// advertiser. Defaults to use active mode.
    fn active(&self, enabled: bool);

    /// Control whether to filter multiple advertisements by the same peer device. Receving
    /// can be useful for some applications. E.g. when using scan to collect information from
    /// beacons that update data frequently. Defaults to filter duplicate advertisements.
    fn filter_duplicates(&self, enabled: bool);

    /// Stops scanning for BLE devices.
    fn stop_scan(&self) -> Result<()>;

    /// Returns the list of [`Peripherals`](trait.Peripheral.html) that have been discovered so far.
    /// Note that this list may contain peripherals that are no longer available.
    fn peripherals(&self) -> Vec<P>;

    /// Returns a particular [`Peripheral`](trait.Peripheral.html) by its address if it has been
    /// discovered.
    fn peripheral(&self, address: BDAddr) -> Option<P>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_addr() {
        let values = vec![
            (
                "2A:00:AA:BB:CC:DD",
                Ok(BDAddr {
                    address: [0xDD, 0xCC, 0xBB, 0xAA, 0x00, 0x2A],
                }),
            ),
            ("2A:00:00", Err(ParseBDAddrError::IncorrectByteCount)),
            ("2A:00:AA:BB:CC:ZZ", Err(ParseBDAddrError::InvalidInt)),
        ];

        for (input, expected) in values {
            let result: ParseBDAddrResult<BDAddr> = input.parse();
            assert_eq!(result, expected);

            if let Ok(uuid) = result {
                assert_eq!(input, uuid.to_string());
            }
        }
    }
}
