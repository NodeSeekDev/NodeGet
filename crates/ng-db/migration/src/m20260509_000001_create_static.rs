use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Static::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Static::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Static::Name).string().not_null())
                    .col(ColumnDef::new(Static::Path).string().not_null())
                    .col(
                        ColumnDef::new(Static::IsHttpRoot)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Static::Cors)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-static-name-unique")
                    .table(Static::Table)
                    .col(Static::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // 使用原生 SQL 创建 partial unique index，
        // 从数据库层强制保证同一时刻只有一条 is_http_root = true 的记录，
        // 防止应用层检查-插入之间的并发竞态（TOCTOU）
        let backend = manager.get_database_backend();
        let sql = match backend {
            sea_orm::DatabaseBackend::Sqlite => {
                r#"CREATE UNIQUE INDEX IF NOT EXISTS "idx-static-is-http-root-unique" ON "static" ("is_http_root") WHERE "is_http_root" = 1"#
            }
            sea_orm::DatabaseBackend::Postgres => {
                r#"CREATE UNIQUE INDEX IF NOT EXISTS "idx-static-is-http-root-unique" ON "static" ("is_http_root") WHERE "is_http_root" = TRUE"#
            }
            sea_orm::DatabaseBackend::MySql => {
                // MySQL 不支持 partial index，此处跳过；应用层检查作为最后防线
                return Ok(());
            }
            _ => {
                // 未知后端：放弃数据库层保护，依赖应用层检查
                return Ok(());
            }
        };
        manager.get_connection().execute_unprepared(sql).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 先尝试删掉 partial index（MySQL 没创建，IF EXISTS 兜底）
        let backend = manager.get_database_backend();
        if matches!(
            backend,
            sea_orm::DatabaseBackend::Sqlite | sea_orm::DatabaseBackend::Postgres
        ) {
            let sql = r#"DROP INDEX IF EXISTS "idx-static-is-http-root-unique""#;
            let _ = manager.get_connection().execute_unprepared(sql).await;
        }
        manager
            .drop_table(Table::drop().table(Static::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Static {
    #[sea_orm(iden = "static")]
    Table,
    Id,
    Name,
    Path,
    IsHttpRoot,
    Cors,
}
