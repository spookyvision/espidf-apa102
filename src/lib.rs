#![feature(generic_const_exprs)]
use std::{mem::size_of, ptr::null_mut};

use bytemuck::{Pod, Zeroable};
use esp_idf_sys::{
    spi_bus_add_device, spi_bus_config_t, spi_bus_config_t__bindgen_ty_1,
    spi_bus_config_t__bindgen_ty_2, spi_bus_config_t__bindgen_ty_3, spi_bus_config_t__bindgen_ty_4,
    spi_bus_initialize, spi_common_dma_t_SPI_DMA_CH_AUTO, spi_device_get_trans_result,
    spi_device_handle_t, spi_device_interface_config_t, spi_device_queue_trans, spi_host_device_t,
    spi_host_device_t_SPI2_HOST, spi_transaction_t, spi_transaction_t__bindgen_ty_1,
    SPICOMMON_BUSFLAG_MASTER,
};
use rgb::RGB8;

pub const DEFAULT_SPI_HOST: spi_host_device_t = spi_host_device_t_SPI2_HOST;
pub const LED_STRIP_SPI_FRAME_SK9822_LED_MSB3: u8 = 0xE0;

#[derive(Pod, Zeroable, Clone, Copy)]
#[repr(C)]
pub struct Pixel {
    brightness: u8,
    b: u8,
    g: u8,
    r: u8,
}

impl From<RGB8> for Pixel {
    fn from(rgb: RGB8) -> Self {
        Self {
            brightness: 10,
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        }
    }
}

impl Default for Pixel {
    fn default() -> Self {
        Self {
            brightness: 10,
            r: Default::default(),
            g: Default::default(),
            b: Default::default(),
        }
    }
}

impl Pixel {
    pub fn new(r: u8, g: u8, b: u8, brightness: u8) -> Self {
        let brightness = if brightness >= 100 {
            31
        } else if brightness > 8 {
            (brightness - 7) / 3
        } else if brightness > 0 {
            1
        } else {
            0
        };
        Self {
            brightness: LED_STRIP_SPI_FRAME_SK9822_LED_MSB3 | (brightness & ((1 << 5) - 1)),
            r,
            g,
            b,
        }
    }
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Data<const N: usize>
where
    [(); N / 16 + 1]:,
{
    start: [u8; 4],
    pixels: [Pixel; N],
    reset: [u8; 4],
    end: [u8; N / 16 + 1],
}

impl<const N: usize> Data<N>
where
    [(); N / 16 + 1]:,
{
    pub fn new() -> Self {
        Self {
            start: Default::default(),
            pixels: [Pixel::default(); N], // grr, no `Default::default()`
            reset: Default::default(),
            end: [0; N / 16 + 1], // grr, no `Default::default()`
        }
    }
}

impl<const N: usize> Default for Data<N>
where
    [(); N / 16 + 1]:,
{
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<const N: usize> Zeroable for Data<N>
where
    [(); N / 16 + 1]:,
{
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

unsafe impl<const N: usize> Pod for Data<N> where [(); N / 16 + 1]: {}

pub struct HeapData {
    length: usize,
    data: Vec<u8>,
}

impl HeapData {
    pub fn new(length: usize) -> Self {
        let payload = vec![0; 4 + length * size_of::<Pixel>() + 4 + length / 16 + 1];
        let mut res = Self {
            length,
            data: payload,
        };
        for i in 0..length {
            res.set_pixel(i, Pixel::default());
        }
        res
    }

    pub fn data(&self) -> &[u8] {
        self.data.as_ref()
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    pub fn set_pixel(&mut self, idx: usize, pixel: Pixel) {
        let one_px = size_of::<Pixel>();
        let offset = 4 + idx * one_px;
        self.data[offset..][..one_px].clone_from_slice(pixel.as_bytes());
    }

    pub fn length(&self) -> usize {
        self.length
    }
}

pub struct Config {
    length: usize,
    data_pin: i32,
    clock_pin: i32,
    clock_speed: i32,
    transfer_size: i32,
    spi_host: spi_host_device_t,
    queue_size: i32,
    dma_channel: u32,
}

impl Config {
    pub fn new(data_pin: i32, clock_pin: i32) -> Self {
        Self {
            length: 512, // TODO make configurable (power-of-two buffer size?)
            data_pin,
            clock_pin,
            clock_speed: 10_000_000,
            transfer_size: 0,           // TODO make configurable
            spi_host: DEFAULT_SPI_HOST, // TODO make configurable
            queue_size: 1,
            dma_channel: spi_common_dma_t_SPI_DMA_CH_AUTO,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new(7, 6)
    }
}

pub struct Apa {
    data: HeapData,
    handle: SpiDevice,
}

struct SpiDevice(spi_device_handle_t);
unsafe impl Send for SpiDevice {}

impl Apa {
    pub fn new(config: Config) -> Self {
        const UNUSED: i32 = -1;

        let data = HeapData::new(config.length);

        let data_out_pin = spi_bus_config_t__bindgen_ty_1 {
            mosi_io_num: config.data_pin,
        };
        let data_in_pin_unused = spi_bus_config_t__bindgen_ty_2 {
            miso_io_num: UNUSED,
        };
        let moar_unused = spi_bus_config_t__bindgen_ty_3 {
            quadwp_io_num: UNUSED,
        };
        let moooaaaaaaar_unused = spi_bus_config_t__bindgen_ty_4 {
            quadhd_io_num: UNUSED,
        };
        let spi_bus_config = spi_bus_config_t {
            __bindgen_anon_1: data_out_pin,
            __bindgen_anon_2: data_in_pin_unused,
            sclk_io_num: config.clock_pin,
            max_transfer_sz: config.transfer_size,
            flags: SPICOMMON_BUSFLAG_MASTER,
            intr_flags: 0,
            __bindgen_anon_3: moar_unused,
            __bindgen_anon_4: moooaaaaaaar_unused,
            data4_io_num: UNUSED,
            data5_io_num: UNUSED,
            data6_io_num: UNUSED,
            data7_io_num: UNUSED,
        };

        let mut spi_interface_config = spi_device_interface_config_t::default();
        spi_interface_config.mode = 3;
        spi_interface_config.clock_speed_hz = config.clock_speed;
        spi_interface_config.spics_io_num = -1;
        spi_interface_config.queue_size = config.queue_size;

        let _res = unsafe {
            spi_bus_initialize(
                config.spi_host,
                &spi_bus_config as *const _,
                config.dma_channel,
            )
        };

        let mut handle = null_mut();

        let _res =
            unsafe { spi_bus_add_device(config.spi_host, &spi_interface_config, &mut handle as _) };
        Self {
            data,
            handle: SpiDevice(handle),
        }
    }

    pub fn set_pixel(&mut self, idx: usize, pixel: Pixel) {
        self.data.set_pixel(idx, pixel);
    }

    pub fn flush(&self) {
        let mut tx = spi_transaction_t::default();

        let txl = 8 * self.data.data().len();
        tx.length = txl as u32;

        let tx_buffer = spi_transaction_t__bindgen_ty_1 {
            tx_buffer: self.data.as_ptr() as _,
        };
        tx.__bindgen_anon_1 = tx_buffer;
        #[allow(non_snake_case)] // throw some shade
        let freeRTOS_magic_copypasta_portMAX_DELAY = 0xffffffff;
        let _res = unsafe {
            spi_device_queue_trans(
                self.handle.0 as _,
                &mut tx as _,
                freeRTOS_magic_copypasta_portMAX_DELAY,
            )
        };

        let mut tx_res = null_mut();
        let _res = unsafe {
            spi_device_get_trans_result(
                self.handle.0 as _,
                &mut tx_res as *mut _,
                freeRTOS_magic_copypasta_portMAX_DELAY,
            )
        };
    }
}
