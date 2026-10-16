/// A generic counter with a default value type.
///
/// Use [`Self::new`] to construct a counter. Its public [`Self::value`] field
/// preserves a Unicode label: café.
#[derive(Clone, Debug)]
pub struct Counter<T: Clone = u32> {
    /// The current value.
    pub value: T,
}

/// Construction and inherent access to [`Counter`].
impl<T> Counter<T>
where
    T: Clone,
{
    /// Construct a value.
    ///
    /// ```rust
    /// # use rust_demo::Counter;
    /// let counter = Counter::new(5_u32);
    /// assert_eq!(counter.read(), 5);
    /// ```
    pub fn new(value: T) -> Self {
        Self { value }
    }

    /// Read through an inherent method, distinct from [`crate::Named::read`].
    pub fn read(&self) -> T {
        self.value.clone()
    }
}

/// A concrete implementation of [`crate::Named`] for [`Counter`].
impl crate::Named for Counter<u32> {
    type Value = u32;
    const LABEL: &'static str = "counter";
    /// Read through the implemented trait.
    fn read(&self) -> u32 {
        self.value
    }
}
