pub mod state {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "ciyi_game_state")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub group_id: i64,
        pub target_word: String,
        pub last_start_time: i64,
        #[sea_orm(column_type = "Text")]
        pub global_history: String,
        #[sea_orm(column_type = "Text")]
        pub current_guesses: String,
        #[sea_orm(column_type = "Text")]
        pub words_rank_list: String,
        #[sea_orm(column_type = "Text")]
        pub hints: String,
        pub is_finished: bool,
        pub direct_guess_enabled: bool,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod record {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "ciyi_win_record")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub group_id: i64,
        pub user_id: i64,
        pub username: String,
        pub timestamp: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
