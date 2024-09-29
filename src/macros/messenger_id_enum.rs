/// Match against the enum name and a list of variant bindings like (Variant = value)
#[macro_export]
macro_rules! messenger_id_enum {
    ($name:ident { $($variant:ident = $value:expr),+ $(,)? }) => {
        #[repr(u16)]
        #[derive(PartialEq, Eq, Debug, Clone, Copy)]
        pub enum $name {
            $($variant = $value),+
        }

        impl $name {
            pub const fn from_u16(value: u16) -> Self {
                match value {
                    $( $value => $name::$variant, )+
                    _ => panic!(),
                }
            }

            pub const fn to_u16(self) -> u16 {
                self as u16
            }
        }


        impl From<$name> for u16 {
            fn from(value: $name) -> Self {
                value.to_u16()
            }
        }

        impl From<u16> for $name {
            fn from(value: u16) -> Self {
                $name::from_u16(value)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_messenger_id_enum() {
        messenger_id_enum!(
            TestEnum {
                VariantA = 1,
                VariantB = 2,
            }
        );

        assert_eq!(TestEnum::VariantA, TestEnum::from_u16(1));
        assert_eq!(TestEnum::VariantB, TestEnum::from_u16(2));
        assert_eq!(1, u16::from(TestEnum::VariantA));
        assert_eq!(2, u16::from(TestEnum::VariantB));
        const X: u16 = TestEnum::VariantA.to_u16();
        assert_eq!(1, X);
        const U16: u16 = 1;
        assert_eq!(TestEnum::from_u16(U16), TestEnum::VariantA);
    }

    #[test]
    fn test_cast_from_zero() {
        messenger_id_enum!(
            TestEnum {
                VariantA = 1,
                VariantB = 2,
            }
        );

        let mut data: [u8; 2] = [0; 2];
        let enm_ptr = data.as_mut_ptr() as *mut TestEnum;
        unsafe {
            // let enm = &mut *enm_ptr; // casting to wrong enum value is UB
            *enm_ptr = TestEnum::VariantA;
            assert_eq!(*enm_ptr, TestEnum::VariantA);
            assert_ne!(*enm_ptr, TestEnum::VariantB);
        }
    }
}
