use sea_orm_migration::prelude::*;

// 迁移名称派生宏
#[derive(DeriveMigrationName)]
pub struct Migration;

// 元数据表的创建和删除迁移实现
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // 执行迁移：创建元数据表
    // 
    // 该函数创建一个名为 metadata 的表，包含以下列：
    // - id: 主键，自增整数
    // - key: 元数据键，唯一索引
    // - value: 元数据值，JSON 格式
    // 
    // # 参数
    // * `manager` - 模式管理器
    // 
    // # 返回值
    // 成功时返回 Ok(())，失败时返回数据库错误
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MetadataInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MetadataInDatabase::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(MetadataInDatabase::Key)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(MetadataInDatabase::Value)
                            .json_binary()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    // 回滚迁移：删除元数据表
    // 
    // # 参数
    // * `manager` - 模式管理器
    // 
    // # 返回值
    // 成功时返回 Ok(())，失败时返回数据库错误
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MetadataInDatabase::Table).to_owned())
            .await
    }
}

// 元数据表的标识符枚举，用于定义表和列的名称
#[derive(DeriveIden)]
enum MetadataInDatabase {
    #[sea_orm(iden = "metadata")]
    Table,
    Id,
    Key,
    Value,
}
