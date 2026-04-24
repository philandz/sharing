pub mod pb {
    pub mod service {
        pub mod sharing {
            tonic::include_proto!("service.sharing");
        }
        pub mod budget {
            tonic::include_proto!("service.budget");
        }
    }
    pub mod common {
        pub mod base {
            tonic::include_proto!("common.base");
        }
    }
    pub mod shared {
        pub mod organization {
            tonic::include_proto!("shared.organization");
        }
        pub mod user {
            tonic::include_proto!("shared.user");
        }
    }
}

pub mod converters;
pub mod handler;
pub mod manager;
