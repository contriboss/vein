use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CachedAssets::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(CachedAssets::Kind).string().not_null())
                    .col(ColumnDef::new(CachedAssets::Name).string().not_null())
                    .col(ColumnDef::new(CachedAssets::Version).string().not_null())
                    .col(ColumnDef::new(CachedAssets::Platform).string())
                    .col(ColumnDef::new(CachedAssets::Path).string().not_null())
                    .col(ColumnDef::new(CachedAssets::Sha256).string().not_null())
                    .col(ColumnDef::new(CachedAssets::SizeBytes).integer().not_null())
                    .col(
                        ColumnDef::new(CachedAssets::LastAccessed)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(
                        Index::create()
                            .col(CachedAssets::Kind)
                            .col(CachedAssets::Name)
                            .col(CachedAssets::Version)
                            .col(CachedAssets::Platform),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CachedAssets::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum CachedAssets {
    Table,
    Kind,
    Name,
    Version,
    Platform,
    Path,
    Sha256,
    SizeBytes,
    LastAccessed,
}
