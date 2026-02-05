pub use sea_orm_migration::prelude::*;

// 迁移管理器结构体，用于管理和执行数据库迁移
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    // 获取所有迁移脚本列表
    // 
    // # 返回值
    // 返回实现了 MigrationTrait 的迁移脚本向量
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260113_044428_create_static_monitoring::Migration),
            Box::new(m20260115_131325_create_dynamic_monitoring::Migration),
            Box::new(m20260118_030100_create_task::Migration),
            Box::new(m20260131_112949_create_token::Migration),
            Box::new(m20260205_024306_create_metadata::Migration),
        ]
    }
}
// 静态监控表创建迁移模块
mod m20260113_044428_create_static_monitoring;
// 动态监控表创建迁移模块
mod m20260115_131325_create_dynamic_monitoring;
// 任务表创建迁移模块
mod m20260118_030100_create_task;
// 令牌表创建迁移模块
mod m20260131_112949_create_token;
// 元数据表创建迁移模块
mod m20260205_024306_create_metadata;
