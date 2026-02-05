use sea_orm_migration::prelude::*;

// 迁移名称派生宏
#[derive(DeriveMigrationName)]
pub struct Migration;

// 令牌数据表的创建和删除迁移实现
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // 执行迁移：创建令牌表
    //
    // 该函数创建一个名为 token 的表，包含以下列：
    // - id: 主键，自增大整数
    // - version: 令牌版本号
    // - token_key: 令牌密钥，唯一索引
    // - token_hash: 令牌哈希值
    // - time_stamp_from: 令牌生效时间戳，可为空
    // - time_stamp_to: 令牌过期时间戳，可为空
    // - token_limit: 令牌权限限制，JSON 格式
    // - username: 用户名，唯一索引，可为空
    // - password_hash: 密码哈希值，可为空
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
                    .table(TokenInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TokenInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::Version)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TokenKey)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TokenHash)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TimeStampFrom)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TimeStampTo)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TokenLimit)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::Username)
                            .string()
                            .unique_key()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::PasswordHash)
                            .string()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    // 回滚迁移：删除令牌表
    //
    // # 参数
    // * `manager` - 模式管理器
    //
    // # 返回值
    // 成功时返回 Ok(())，失败时返回数据库错误
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TokenInDatabase::Table).to_owned())
            .await
    }
}

// 令牌表的标识符枚举，用于定义表和列的名称
#[derive(DeriveIden)]
enum TokenInDatabase {
    #[sea_orm(iden = "token")]
    Table,
    Id,
    Version,
    TokenKey,
    TokenHash,
    TimeStampFrom,
    TimeStampTo,
    TokenLimit,
    Username,
    PasswordHash,
}
