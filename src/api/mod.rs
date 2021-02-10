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
mod bdaddr;
pub mod bleuuid;

pub use adapter_manager::AdapterManager;

use crate::Result;
use async_trait::async_trait;
use bitflags::bitflags;
use futures::stream::Stream;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "serde")]
use serde_cr as serde;
use std::{
    collections::{BTreeSet, HashMap},
    fmt::{self, Debug, Display, Formatter},
    pin::Pin,
};
use uuid::Uuid;

pub use self::bdaddr::{BDAddr, ParseBDAddrError};

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

impl Default for CharPropFlags {
    fn default() -> Self {
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
#[async_trait]
pub trait Peripheral: Send + Sync + Clone + Debug {
    /// Returns the MAC address of the peripheral.
    fn address(&self) -> BDAddr;

    /// Returns the set of properties associated with the peripheral. These may be updated over time
    /// as additional advertising reports are received.
    async fn properties(&self) -> Result<PeripheralProperties>;

    /// The set of characteristics we've discovered for this device. This will be empty until
    /// `discover_characteristics` is called.
    fn characteristics(&self) -> BTreeSet<Characteristic>;

    /// Returns true iff we are currently connected to the device.
    async fn is_connected(&self) -> Result<bool>;

    /// Creates a connection to the device. If this method returns Ok there has been successful
    /// connection. Note that peripherals allow only one connection at a time. Operations that
    /// attempt to communicate with a device will fail until it is connected.
    async fn connect(&self) -> Result<()>;

    /// Terminates a connection to the device.
    async fn disconnect(&self) -> Result<()>;

    /// Discovers all characteristics for the device.
    async fn discover_characteristics(&self) -> Result<Vec<Characteristic>>;

    /// Write some data to the characteristic. Returns an error if the write couldn't be sent or (in
    /// the case of a write-with-response) if the device returns an error.
    async fn write(
        &self,
        characteristic: &Characteristic,
        data: &[u8],
        write_type: WriteType,
    ) -> Result<()>;

    /// Sends a read request to the device. Returns either an error if the request was not accepted
    /// or the response from the device.
    async fn read(&self, characteristic: &Characteristic) -> Result<Vec<u8>>;

    /// Enables either notify or indicate (depending on support) for the specified characteristic.
    async fn subscribe(&self, characteristic: &Characteristic) -> Result<()>;

    /// Disables either notify or indicate (depending on support) for the specified characteristic.
    async fn unsubscribe(&self, characteristic: &Characteristic) -> Result<()>;

    /// Returns a stream of notifications for characteristic value updates. The stream will receive
    /// a notification when a value notification or indication is received from the device. This
    /// method should only be used after a connection has been established.
    async fn notifications(&self) -> Result<Pin<Box<dyn Stream<Item = ValueNotification>>>>;
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
        manufacturer_data: HashMap<u16, Vec<u8>>,
    },
    /// Emitted when a Service Data advertisement has been received from a device
    ServiceDataAdvertisement {
        address: BDAddr,
        service_data: HashMap<Uuid, Vec<u8>>,
    },
    /// Emitted when the advertised services for a device has been updated
    ServicesAdvertisement {
        address: BDAddr,
        services: Vec<Uuid>,
    },
}

/// Central is the "client" of BLE. It's able to scan for and establish connections to peripherals.
#[async_trait]
pub trait Central: Send + Sync + Clone {
    type Peripheral: Peripheral;

    /// Retreive a stream of `CentralEvent`s. This stream will receive notifications when events
    /// occur for this Central module. See [`CentralEvent`](enum.CentralEvent.html) for the full set
    /// of possible events.
    async fn events(&self) -> Result<Pin<Box<dyn Stream<Item = CentralEvent>>>>;

    /// Starts a scan for BLE devices. This scan will generally continue until explicitly stopped,
    /// although this may depend on your Bluetooth adapter. Discovered devices will be announced
    /// to subscribers of `events` and will be available via `peripherals()`.
    async fn start_scan(&self) -> Result<()>;

    /// Control whether to use active or passive scan mode to find BLE devices. Active mode scan
    /// notifies advertisers about the scan, whereas passive scan only receives data from the
    /// advertiser. Defaults to use active mode.
    async fn active(&self, enabled: bool);

    /// Control whether to filter multiple advertisements by the same peer device. Duplicates can be
    /// useful for some applications, e.g. when using a scan to collect information from beacons
    /// that update data frequently. Defaults to filter duplicate advertisements.
    async fn filter_duplicates(&self, enabled: bool);

    /// Stops scanning for BLE devices.
    async fn stop_scan(&self) -> Result<()>;

    /// Returns the list of [`Peripherals`](trait.Peripheral.html) that have been discovered so far.
    /// Note that this list may contain peripherals that are no longer available.
    async fn peripherals(&self) -> Result<Vec<Self::Peripheral>>;

    /// Returns a particular [`Peripheral`](trait.Peripheral.html) by its address if it has been
    /// discovered.
    async fn peripheral(&self, address: BDAddr) -> Result<Self::Peripheral>;
}
