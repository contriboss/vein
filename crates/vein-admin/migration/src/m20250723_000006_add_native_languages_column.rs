use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(GemMetadata::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(GemMetadata::NativeLanguagesJson).string(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(GemMetadata::Table)
                    .drop_column(GemMetadata::NativeLanguagesJson)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum GemMetadata {
    Table,
    NativeLanguagesJson,
}
