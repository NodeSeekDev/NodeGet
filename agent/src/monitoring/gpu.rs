use crate::monitoring::get_global_gpu;
use nodeget_lib::monitoring::data_structure::{DynamicGpuData, StaticGpuData};
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use tokio::sync::{Mutex, MutexGuard, OnceCell};

// 从 GPU 获取的静态数据结构，包含 GPU 的基本信息
#[derive(Debug)]
pub struct StaticDataFromGpu(pub Vec<StaticGpuData>);

// 全局静态 GPU 数据实例，用于缓存 GPU 静态信息
static GLOBAL_STATIC_DATA_FROM_GPU: OnceCell<Mutex<StaticDataFromGpu>> = OnceCell::const_new();

impl StaticDataFromGpu {
    // 创建新的静态 GPU 数据实例
    //
    // 该函数通过 NVML 获取 GPU 的静态信息，如设备 ID、名称、CUDA 核心数和架构
    //
    // # 返回值
    // 返回包含所有 GPU 静态数据的向量
    pub async fn new() -> Self {
        let nvml_mutex = get_global_gpu().await;
        let nvml_guard = nvml_mutex.lock().await;

        let Some(nvml) = &*nvml_guard else {
            return Self(vec![]);
        };

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

        Self(data)
    }

    // 获取静态 GPU 数据的可变引用
    //
    // 如果全局静态 GPU 数据实例不存在，则初始化它；否则直接返回现有的实例
    //
    // # 返回值
    // 返回静态 GPU 数据的互斥锁保护的可变引用
    pub async fn get() -> MutexGuard<'static, Self> {
        let data_mutex = GLOBAL_STATIC_DATA_FROM_GPU
            .get_or_init(|| async { Mutex::new(Self::new().await) })
            .await;

        data_mutex.lock().await
    }
}

// 从 GPU 获取的动态数据结构，包含 GPU 的实时性能数据
#[derive(Debug)]
pub struct DynamicDataFromGpu(pub Vec<DynamicGpuData>);

// 全局动态 GPU 数据实例，用于缓存 GPU 动态信息
static GLOBAL_DYNAMIC_DATA_FROM_GPU: OnceCell<Mutex<DynamicDataFromGpu>> = OnceCell::const_new();

impl DynamicDataFromGpu {
    // 创建新的动态 GPU 数据实例
    //
    // 该函数通过 NVML 获取 GPU 的动态信息，如显存使用情况、利用率、温度和时钟频率
    //
    // # 返回值
    // 返回包含所有 GPU 动态数据的向量
    async fn new() -> Self {
        let nvml_mutex = get_global_gpu().await;
        let data = {
            let nvml_guard = nvml_mutex.lock().await;

            let Some(nvml) = &*nvml_guard else {
                return Self(vec![]);
            };

            let gpu_count = nvml.device_count().unwrap_or(0);

            (0..gpu_count)
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
                        temperature: device
                            .temperature(TemperatureSensor::Gpu)
                            .ok()?
                            .try_into()
                            .ok()?,
                    })
                })
                .collect::<Vec<_>>()
        };

        Self(data)
    }

    // 更新动态 GPU 数据
    //
    // 该函数刷新现有 GPU 数据，更新显存使用情况、利用率、温度和时钟频率等信息
    async fn update(&mut self) {
        let nvml_mutex = get_global_gpu().await;
        {
            let nvml_guard = nvml_mutex.lock().await;

            let Some(nvml) = &*nvml_guard else { return };

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
    }

    // 异步刷新并获取动态 GPU 数据
    //
    // 如果全局动态 GPU 数据实例不存在，则初始化它；否则更新现有数据并返回
    //
    // # 返回值
    // 返回动态 GPU 数据的互斥锁保护的可变引用
    pub async fn refresh_and_get() -> MutexGuard<'static, Self> {
        let data_mutex = GLOBAL_DYNAMIC_DATA_FROM_GPU
            .get_or_init(|| async { Mutex::new(Self::new().await) })
            .await;

        let mut data = data_mutex.lock().await;
        data.update().await;

        data
    }
}
