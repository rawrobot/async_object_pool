pub trait Resettable {
    fn reset(&mut self);
}

impl<T> Resettable for Option<T>
where
    T: Resettable,
{
    fn reset(&mut self) {
        if let Some(x) = self {
            x.reset();
        }
    }
}

impl<T1, T2> Resettable for (T1, T2)
where
    T1: Resettable,
    T2: Resettable,
{
    fn reset(&mut self) {
        self.0.reset();
        self.1.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Dummy {
        value: i32,
    }

    impl Resettable for Dummy {
        fn reset(&mut self) {
            self.value = 0;
        }
    }

    #[test]
    fn test_resettable_for_struct() {
        let mut d = Dummy { value: 42 };
        d.reset();
        assert_eq!(d.value, 0);
    }

    #[test]
    fn test_resettable_for_option() {
        let mut d = Some(Dummy { value: 10 });
        d.reset();
        assert_eq!(d, Some(Dummy { value: 0 }));

        let mut none: Option<Dummy> = None;
        none.reset(); // Should not panic or do anything
        assert_eq!(none, None);
    }

    #[test]
    fn test_resettable_for_tuple() {
        let mut t = (Dummy { value: 5 }, Dummy { value: 7 });
        t.reset();
        assert_eq!(t, (Dummy { value: 0 }, Dummy { value: 0 }));
    }
}
