use nodeget_lib::monitoring::data_structure::{DynamicGpuData, StaticGpuData};
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use parking_lot::{Mutex, MutexGuard};
use tokio::sync::OnceCell;
use crate::monitoring::get_global_gpu;

#[derive(Debug)]
pub struct StaticDataFromGpu(pub Vec<StaticGpuData>);

static GLOBAL_STATIC_DATA_FROM_GPU: OnceCell<Mutex<StaticDataFromGpu>> = OnceCell::const_new();

impl StaticDataFromGpu {
    pub async fn new() -> StaticDataFromGpu {
        let nvml_mutex = get_global_gpu().await;
        let nvml = nvml_mutex.lock();

        let gpu_count = nvml.device_count().unwrap_or(0);

        let data = (0..gpu_count)
            .filter_map(|id| {
                let device = nvml.device_by_index(id).ok()?;
                Some(StaticGpuData {
                    id: id + 1,
                    name: device.name().ok()?,
                    cuda_cores: u64::from(device.num_cores().ok()?),
                    architecture: device.architecture().ok()?.to_string(),
                })
            })
            .collect::<Vec<_>>();

        StaticDataFromGpu(data)
    }

    pub async fn get() -> MutexGuard<'static, StaticDataFromGpu> {
        let data_mutex = GLOBAL_STATIC_DATA_FROM_GPU
            .get_or_init(|| async {
                Mutex::new(StaticDataFromGpu::new().await)
            })
            .await;

        data_mutex.lock()
    }
}

#[derive(Debug)]
pub struct DynamicDataFromGpu(pub Vec<DynamicGpuData>);

static GLOBAL_DYNAMIC_DATA_FROM_GPU: OnceCell<Mutex<DynamicDataFromGpu>> = OnceCell::const_new();

impl DynamicDataFromGpu {
    async fn new() -> DynamicDataFromGpu {
        let nvml_mutex = get_global_gpu().await;
        let nvml = nvml_mutex.lock();

        let gpu_count = nvml.device_count().unwrap_or(0);

        let data = (0..gpu_count)
            .filter_map(|id| {
                let device = nvml.device_by_index(id).ok()?;
                let memory_usage = device.memory_info().ok()?;
                let utilization = device.utilization_rates().ok()?;

                Some(DynamicGpuData {
                    id: id + 1,
                    used_memory: memory_usage.used,
                    total_memory: memory_usage.total,
                    graphics_clock_mhz: device.clock_info(Clock::Graphics).ok()?.into(),
                    sm_clock_mhz: device.clock_info(Clock::SM).ok()?.into(),
                    memory_clock_mhz: device.clock_info(Clock::Memory).ok()?.into(),
                    video_clock_mhz: device.clock_info(Clock::Video).ok()?.into(),
                    utilization_gpu: utilization.gpu.try_into().ok()?,
                    utilization_memory: utilization.memory.try_into().ok()?,
                    temperature: device.temperature(TemperatureSensor::Gpu).ok()?.try_into().ok()?,
                })
            })
            .collect::<Vec<_>>();

        DynamicDataFromGpu(data)
    }

    async fn update(&mut self) {
        let nvml_mutex = get_global_gpu().await;
        let nvml = nvml_mutex.lock();

        for gpu_data in &mut self.0 {
            let index = gpu_data.id.saturating_sub(1);

            if let Ok(device) = nvml.device_by_index(index) {
                if let Ok(memory_usage) = device.memory_info() {
                    gpu_data.used_memory = memory_usage.used;
                    gpu_data.total_memory = memory_usage.total;
                }

                if let Ok(utilization) = device.utilization_rates() {
                    gpu_data.utilization_gpu = utilization.gpu as _;
                    gpu_data.utilization_memory = utilization.memory as _;
                }

                if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
                    gpu_data.temperature = temp as _;
                }

                if let Ok(clock) = device.clock_info(Clock::Graphics) {
                    gpu_data.graphics_clock_mhz = clock.into();
                }
                if let Ok(clock) = device.clock_info(Clock::SM) {
                    gpu_data.sm_clock_mhz = clock.into();
                }
                if let Ok(clock) = device.clock_info(Clock::Memory) {
                    gpu_data.memory_clock_mhz = clock.into();
                }
                if let Ok(clock) = device.clock_info(Clock::Video) {
                    gpu_data.video_clock_mhz = clock.into();
                }
            }
        }
    }
    
    pub async fn refresh_and_get() -> MutexGuard<'static, DynamicDataFromGpu> {
        let data_mutex = GLOBAL_DYNAMIC_DATA_FROM_GPU
            .get_or_init(|| async {
                Mutex::new(DynamicDataFromGpu::new().await)
            })
            .await;

        let mut data = data_mutex.lock();
        data.update().await;

        data
    }
}