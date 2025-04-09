use crate::{controller::BaseError, database::get_connection, db_execute, db_object};

db_object! {
    #[derive(Queryable, Insertable, Debug)]
    #[diesel(table_name = price)]
    pub struct Price {
        pub id: i64,
        pub model_id: i64,
        pub start_time: i64,
        pub currency: String,
        pub input_price: i32,
        pub output_price: i32,
        pub input_cache_price: i32,
        pub output_cache_price: i32,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

impl Price {
    pub fn query_one(id: i64) -> Price {
        let conn = &mut get_connection();

        db_execute!(conn, {
            price::table
                .filter(price::dsl::id.eq(id))
                .first::<PriceDb>(conn)
                .map_err(|_| BaseError::NotFound(None))
                .unwrap()
                .from_db()
        })
    }

    pub fn insert_one(data: Price) {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let data = PriceDb::to_db(&data);

            diesel::insert_into(price::table)
                .values(&data)
                .execute(conn)
                .unwrap();
        })
    }
}
