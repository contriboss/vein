use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CatalogGems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CatalogGems::Name)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CatalogGems::LatestVersion).string())
                    .col(
                        ColumnDef::new(CatalogGems::SyncedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CatalogGems::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum CatalogGems {
    Table,
    Name,
    LatestVersion,
    SyncedAt,
}
