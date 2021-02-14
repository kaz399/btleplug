fn main() {
    #[cfg(target_os = "windows")]
    windows::build!(
        windows::devices::bluetooth::generic_attribute_profile::{
            GattCharacteristic,
            GattCharacteristicProperties,
            GattClientCharacteristicConfigurationDescriptorValue,
            GattCommunicationStatus,
            GattDeviceService,
            GattDeviceServicesResult,
            GattValueChangedEventArgs,
        },
        windows::devices::bluetooth::advertisement::*,
        windows::devices::bluetooth::{
            BluetoothConnectionStatus,
            BluetoothLEDevice,
        },
        windows::devices::radios::{
            Radio,
            RadioKind
        },
        windows::storage::streams::{
            DataReader,
            DataWriter,
        },
    );
}
