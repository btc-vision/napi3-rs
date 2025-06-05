use std::ffi::{c_void, CString};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::{self, NonNull};
use std::{mem, slice};

#[cfg(all(debug_assertions, not(windows)))]
use crate::bindgen_prelude::{register_backing_ptr, unregister_backing_ptr};
use crate::{
  bindgen_prelude::{
    FromNapiValue, JsObjectValue, JsValue, This, ToNapiValue, TypeName, ValidateNapiValue,
  },
  check_status, sys, Env, Error, Result, Status, Value, ValueType,
};

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TypedArrayType {
  Int8 = 0,
  Uint8,
  Uint8Clamped,
  Int16,
  Uint16,
  Int32,
  Uint32,
  Float32,
  Float64,
  #[cfg(feature = "napi6")]
  BigInt64,
  #[cfg(feature = "napi6")]
  BigUint64,

  /// compatible with higher versions
  Unknown = 1024,
}

thread_local! {
    pub(crate) static IN_FINALISER: std::cell::Cell<bool> =
        std::cell::Cell::new(false);
}

impl AsRef<str> for TypedArrayType {
  fn as_ref(&self) -> &str {
    match self {
      TypedArrayType::Int8 => "Int8",
      TypedArrayType::Uint8 => "Uint8",
      TypedArrayType::Uint8Clamped => "Uint8Clamped",
      TypedArrayType::Int16 => "Int16",
      TypedArrayType::Uint16 => "Uint16",
      TypedArrayType::Int32 => "Int32",
      TypedArrayType::Uint32 => "Uint32",
      TypedArrayType::Float32 => "Float32",
      TypedArrayType::Float64 => "Float64",
      #[cfg(feature = "napi6")]
      TypedArrayType::BigInt64 => "BigInt64",
      #[cfg(feature = "napi6")]
      TypedArrayType::BigUint64 => "BigUint64",
      TypedArrayType::Unknown => "Unknown",
    }
  }
}

impl From<sys::napi_typedarray_type> for TypedArrayType {
  fn from(value: sys::napi_typedarray_type) -> Self {
    match value {
      sys::TypedarrayType::int8_array => Self::Int8,
      sys::TypedarrayType::uint8_array => Self::Uint8,
      sys::TypedarrayType::uint8_clamped_array => Self::Uint8Clamped,
      sys::TypedarrayType::int16_array => Self::Int16,
      sys::TypedarrayType::uint16_array => Self::Uint16,
      sys::TypedarrayType::int32_array => Self::Int32,
      sys::TypedarrayType::uint32_array => Self::Uint32,
      sys::TypedarrayType::float32_array => Self::Float32,
      sys::TypedarrayType::float64_array => Self::Float64,
      #[cfg(feature = "napi6")]
      sys::TypedarrayType::bigint64_array => Self::BigInt64,
      #[cfg(feature = "napi6")]
      sys::TypedarrayType::biguint64_array => Self::BigUint64,
      _ => Self::Unknown,
    }
  }
}

impl From<TypedArrayType> for sys::napi_typedarray_type {
  fn from(value: TypedArrayType) -> sys::napi_typedarray_type {
    value as i32
  }
}

#[cfg(target_family = "wasm")]
extern "C" {
  fn emnapi_sync_memory(
    env: crate::sys::napi_env,
    js_to_wasm: bool,
    arraybuffer_or_view: crate::sys::napi_value,
    byte_offset: usize,
    length: usize,
  ) -> crate::sys::napi_status;
}

#[derive(Clone, Copy)]
/// Represents a JavaScript ArrayBuffer
pub struct ArrayBuffer<'env> {
  pub(crate) value: Value,
  pub(crate) data: &'env [u8],
}

impl<'env> JsValue<'env> for ArrayBuffer<'env> {
  fn value(&self) -> Value {
    self.value
  }
}

impl<'env> JsObjectValue<'env> for ArrayBuffer<'env> {}

impl FromNapiValue for ArrayBuffer<'_> {
  unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
    let value = Value {
      env,
      value: napi_val,
      value_type: ValueType::Object,
    };
    let mut data = ptr::null_mut();
    let mut byte_length = 0;
    check_status!(unsafe {
      sys::napi_get_arraybuffer_info(env, napi_val, &mut data, &mut byte_length)
    })?;
    Ok(ArrayBuffer {
      value,
      data: if data.is_null() {
        &[]
      } else {
        unsafe { std::slice::from_raw_parts(data as *const u8, byte_length) }
      },
    })
  }
}

impl Deref for ArrayBuffer<'_> {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    self.data
  }
}

impl<'env> ArrayBuffer<'env> {
  /// Create a new `ArrayBuffer` from a `Vec<u8>`.
  pub fn from_data<D: Into<Vec<u8>>>(env: &Env, data: D) -> Result<Self> {
    let mut buf = ptr::null_mut();
    let mut data = data.into();
    let mut inner_ptr = data.as_mut_ptr();
    let len = data.len();

    // Tell V8 how many bytes live outside the JS heap
    let mut _dummy = 0;
    check_status!(
      unsafe { sys::napi_adjust_external_memory(env.0, len as i64, &mut _dummy) },
      "adjust external memory"
    )?;

    let mut status = unsafe {
      let cap = data.capacity();
      sys::napi_create_external_arraybuffer(
        env.0,
        inner_ptr.cast(),
        len,
        Some(finalize_slice::<u8>),
        Box::into_raw(Box::new((len, cap))).cast(),
        &mut buf,
      )
    };

    if status == napi_sys::Status::napi_no_external_buffers_allowed {
      let mut inner_data = unsafe { Vec::from_raw_parts(inner_ptr, len, len) };
      let mut underlying_data = ptr::null_mut();
      status = unsafe { sys::napi_create_arraybuffer(env.0, len, &mut underlying_data, &mut buf) };
      unsafe {
        ptr::copy_nonoverlapping(inner_data.as_mut_ptr(), underlying_data.cast(), len);
      }
      inner_ptr = underlying_data.cast(); // <- this is the real backing store now
    } else {
      mem::forget(data); // JS owns the original Vec’s memory
    }
    check_status!(status, "Failed to create buffer slice from data")?;

    #[cfg(all(debug_assertions, not(windows)))]
    if len != 0 {
      register_backing_ptr(inner_ptr);
    }

    Ok(Self {
      value: Value {
        env: env.0,
        value: buf,
        value_type: ValueType::Object,
      },
      data: if len == 0 {
        &[]
      } else {
        unsafe { slice::from_raw_parts(inner_ptr.cast(), len) }
      },
    })
  }

  /// ## Safety
  ///
  /// Mostly the same with `from_data`
  ///
  /// Provided `finalize_callback` will be called when `[u8]` got dropped.
  ///
  /// You can pass in `noop_finalize` if you have nothing to do in finalize phase.
  ///
  /// ### Notes
  ///
  /// JavaScript may mutate the data passed in to this buffer when writing the buffer.
  /// However, some JavaScript runtimes do not support external buffers (notably electron!)
  /// in which case modifications may be lost.
  ///
  /// If you need to support these runtimes, you should create a buffer by other means and then
  /// later copy the data back out.
  pub unsafe fn from_external<T: 'env, F: FnOnce(Env, T)>(
    env: &Env,
    data: *mut u8,
    len: usize,
    finalize_hint: T,
    finalize_callback: F,
  ) -> Result<Self> {
    if data.is_null() || std::ptr::eq(data, crate::EMPTY_VEC.as_ptr()) {
      return Err(Error::new(
        Status::InvalidArg,
        "Borrowed data should not be null".to_owned(),
      ));
    }

    // Will hold a new buffer if the Electron fallback is taken.
    let mut underlying_data: *mut c_void = ptr::null_mut();

    let hint_ptr = Box::into_raw(Box::new((finalize_hint, finalize_callback)));
    let mut arraybuffer_value = ptr::null_mut();

    let mut status = unsafe {
      sys::napi_create_external_arraybuffer(
        env.0,
        data.cast(),
        len,
        Some(crate::env::raw_finalize_with_custom_callback::<T, F>),
        hint_ptr.cast(),
        &mut arraybuffer_value,
      )
    };

    if status == sys::Status::napi_no_external_buffers_allowed {
      let (hint, finalize) = *Box::from_raw(hint_ptr);
      status = unsafe {
        sys::napi_create_arraybuffer(
          env.0,
          len,
          &mut underlying_data, // now has the expected type
          &mut arraybuffer_value,
        )
      };
      unsafe {
        ptr::copy_nonoverlapping(data, underlying_data.cast::<u8>(), len);
      }
      finalize(*env, hint);
    }

    check_status!(status, "Failed to create arraybuffer from data")?;

    // decide which pointer is really alive
    let backing_ptr = if status == sys::Status::napi_no_external_buffers_allowed {
      underlying_data.cast::<u8>()
    } else {
      data
    };

    #[cfg(all(debug_assertions, not(windows)))]
    register_backing_ptr(backing_ptr);

    Ok(Self {
      value: Value {
        env: env.0,
        value: arraybuffer_value,
        value_type: ValueType::Object,
      },
      data: if len == 0 {
        &[]
      } else {
        unsafe { std::slice::from_raw_parts(backing_ptr, len) }
      },
    })
  }

  /// Copy data from a `&[u8]` and create a `ArrayBuffer` from it.
  pub fn copy_from<D: AsRef<[u8]>>(env: &Env, data: D) -> Result<Self> {
    let data = data.as_ref();
    let len = data.len();
    let mut arraybuffer_value = ptr::null_mut();
    let mut underlying_data = ptr::null_mut();

    check_status!(
      unsafe {
        sys::napi_create_arraybuffer(env.0, len, &mut underlying_data, &mut arraybuffer_value)
      },
      "Failed to create ArrayBuffer"
    )?;

    Ok(Self {
      value: Value {
        env: env.0,
        value: arraybuffer_value,
        value_type: ValueType::Object,
      },
      data: if len == 0 {
        &[]
      } else {
        unsafe { std::slice::from_raw_parts(underlying_data.cast(), len) }
      },
    })
  }

  #[cfg(feature = "napi7")]
  /// Generally, an ArrayBuffer is non-detachable if it has been detached before.
  ///
  /// The engine may impose additional conditions on whether an ArrayBuffer is detachable.
  ///
  /// For example, V8 requires that the ArrayBuffer be external, that is, created with napi_create_external_arraybuffer
  pub fn detach(self) -> Result<()> {
    check_status!(unsafe { sys::napi_detach_arraybuffer(self.value.env, self.value.value) })
  }

  #[cfg(feature = "napi7")]
  /// The ArrayBuffer is considered `detached` if its internal data is null.
  ///
  /// This API represents the invocation of the `ArrayBuffer` `IsDetachedBuffer` operation as defined in [Section 24.1.1.2](https://tc39.es/ecma262/#sec-isdetachedbuffer) of the ECMAScript Language Specification.
  pub fn is_detached(&self) -> Result<bool> {
    let mut is_detached = false;
    check_status!(unsafe {
      sys::napi_is_detached_arraybuffer(self.value.env, self.value.value, &mut is_detached)
    })?;
    Ok(is_detached)
  }
}

trait Finalizer {
  type RustType;

  fn take_finalizer(&mut self) -> Option<Box<dyn FnOnce(*mut Self::RustType, usize)>>;

  fn byte_len(&self) -> usize;
}

macro_rules! impl_typed_array {
  ($name:ident, $rust_type:ident, $typed_array_type:expr) => {
    pub struct $name {
      data: *mut $rust_type,
      length: usize,
      #[allow(unused)]
      byte_offset: usize,
      raw: Option<(crate::sys::napi_ref, crate::sys::napi_env)>,
      finalizer_notify: Option<Box<dyn FnOnce(*mut $rust_type, usize)>>,
      owned_by_rust: bool,
    }

    /// SAFETY: This is undefined behavior, as the JS side may always modify the underlying buffer,
    /// without synchronization. Also see the docs for the `DerfMut` impl.
    #[cfg(feature = "unsafe_send_sync")]
    unsafe impl Send for $name {}
    #[cfg(feature = "unsafe_send_sync")]
    unsafe impl Sync for $name {}

    impl Finalizer for $name {
      type RustType = $rust_type;

      fn take_finalizer(&mut self) -> Option<Box<dyn FnOnce(*mut Self::RustType, usize)>> {
        self.finalizer_notify.take()
      }

      fn byte_len(&self) -> usize {
        self.length * std::mem::size_of::<$rust_type>()
      }
    }

    impl Drop for $name {
      fn drop(&mut self) {
        if self.owned_by_rust {
          self.drop_callback();
        }

        if self.raw.is_none() && self.owned_by_rust {
          if !self.data.is_null() {
            unsafe { Vec::from_raw_parts(self.data, self.length, self.length) };
          }
        }

        /* ------------------------------------------
         *  B. We kept a napi_ref – release it,
         *     unless we are inside the finaliser.
         * -----------------------------------------*/
        let Some((reference, env)) = self.raw else {
          return;
        };

        // A copy created by `to_napi_value(&mut self)` shares the reference
        // but stores `env = null_mut()`.  Let the original clean up.
        if env.is_null() {
          return;
        }

        // Inside a GC finaliser we must NOT touch N-API.
        if crate::bindgen_runtime::IN_FINALISER.with(|f| f.get()) {
          // Leak the reference safely; Node will reclaim it on env teardown.
          return;
        }

        let mut ref_count = 0;
        crate::check_status_or_throw!(
          env,
          unsafe { sys::napi_reference_unref(env, reference, &mut ref_count) },
          "Failed to unref TypedArray"
        );
        debug_assert!(ref_count == 0, "TypedArray ref count not zero");
        crate::check_status_or_throw!(
          env,
          unsafe { sys::napi_delete_reference(env, reference) },
          "Failed to delete TypedArray reference"
        );
      }
    }

    impl $name {
      #[inline(always)]
      fn drop_callback(&mut self) {
        if let Some(cb) = self.finalizer_notify.take() {
          cb(self.data, self.length);
        }
      }

      #[cfg(target_family = "wasm")]
      pub fn sync(&mut self, env: &crate::Env) {
        if let Some((reference, _)) = self.raw {
          let mut value = ptr::null_mut();
          let mut array_buffer = ptr::null_mut();
          crate::check_status_or_throw!(
            env.raw(),
            unsafe { crate::sys::napi_get_reference_value(env.raw(), reference, &mut value) },
            "Failed to get reference value from TypedArray while syncing"
          );
          crate::check_status_or_throw!(
            env.raw(),
            unsafe {
              crate::sys::napi_get_typedarray_info(
                env.raw(),
                value,
                &mut ($typed_array_type as i32) as *mut i32,
                &mut self.length as *mut usize,
                ptr::null_mut(),
                &mut array_buffer,
                &mut self.byte_offset as *mut usize,
              )
            },
            "Failed to get ArrayBuffer under the TypedArray while syncing"
          );
          crate::check_status_or_throw!(
            env.raw(),
            unsafe {
              emnapi_sync_memory(
                env.raw(),
                false,
                array_buffer,
                self.byte_offset,
                self.length,
              )
            },
            "Failed to sync memory"
          );
        } else {
          return;
        }
      }

      pub fn new(mut data: Vec<$rust_type>) -> Self {
        data.shrink_to_fit();
        let ret = $name {
          data: data.as_mut_ptr(),
          length: data.len(),
          byte_offset: 0,
          raw: None,
          finalizer_notify: None,
          owned_by_rust: true,
        };
        mem::forget(data);
        ret
      }

      pub fn with_data_copied<D>(data: D) -> Self
      where
        D: AsRef<[$rust_type]>,
      {
        let mut data_copied = data.as_ref().to_vec();
        let ret = $name {
          data: data_copied.as_mut_ptr(),
          length: data.as_ref().len(),
          finalizer_notify: None,
          owned_by_rust: true,
          raw: None,
          byte_offset: 0,
        };
        mem::forget(data_copied);
        ret
      }

      /// # Safety
      ///
      /// The caller will be notified when the data is deallocated by vm
      pub unsafe fn with_external_data<F>(data: *mut $rust_type, length: usize, notify: F) -> Self
      where
        F: 'static + FnOnce(*mut $rust_type, usize),
      {
        $name {
          data,
          length,
          finalizer_notify: Some(Box::new(notify)),
          raw: None,
          owned_by_rust: true,
          byte_offset: 0,
        }
      }

      /// # Safety
      ///
      /// This is literally undefined behavior, as the JS side may always modify the underlying buffer,
      /// without synchronization. Also see the docs for the `DerefMut` impl.
      pub unsafe fn as_mut(&mut self) -> &mut [$rust_type] {
        if self.data.is_null() {
          return &mut [];
        }

        unsafe { std::slice::from_raw_parts_mut(self.data, self.length) }
      }
    }

    impl Deref for $name {
      type Target = [$rust_type];

      fn deref(&self) -> &Self::Target {
        self.as_ref()
      }
    }

    impl AsRef<[$rust_type]> for $name {
      fn as_ref(&self) -> &[$rust_type] {
        if self.data.is_null() {
          return &[];
        }

        unsafe { std::slice::from_raw_parts(self.data, self.length) }
      }
    }

    impl TypeName for $name {
      fn type_name() -> &'static str {
        concat!("TypedArray<", stringify!($rust_type), ">")
      }

      fn value_type() -> crate::ValueType {
        crate::ValueType::Object
      }
    }

    impl ValidateNapiValue for $name {
      unsafe fn validate(
        env: sys::napi_env,
        napi_val: sys::napi_value,
      ) -> Result<crate::sys::napi_value> {
        let mut is_typed_array = false;
        check_status!(
          unsafe { sys::napi_is_typedarray(env, napi_val, &mut is_typed_array) },
          "Failed to check if value is typed array"
        )?;
        if !is_typed_array {
          return Err(Error::new(
            Status::InvalidArg,
            "Expected a TypedArray value".to_owned(),
          ));
        }
        Ok(ptr::null_mut())
      }
    }

    impl FromNapiValue for $name {
      unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
        let mut typed_array_type = 0;
        let mut length = 0;
        let mut data = ptr::null_mut();
        let mut array_buffer = ptr::null_mut();
        let mut byte_offset = 0;

        check_status!(
          unsafe {
            sys::napi_get_typedarray_info(
              env,
              napi_val,
              &mut typed_array_type,
              &mut length,
              &mut data,
              &mut array_buffer,
              &mut byte_offset,
            )
          },
          "Get TypedArray info failed"
        )?;
        if typed_array_type != $typed_array_type as i32 {
          return Err(Error::new(
            Status::InvalidArg,
            format!(
              "Expected {}, got {}Array",
              stringify!($name),
              TypedArrayType::from(typed_array_type).as_ref()
            ),
          ));
        }
        Ok($name {
          data: data.cast(),
          length,
          byte_offset,
          raw: None,
          owned_by_rust: false,
          finalizer_notify: None,
        })
      }
    }

    impl ToNapiValue for $name {
      unsafe fn to_napi_value(env: sys::napi_env, mut val: Self) -> Result<sys::napi_value> {
        if let Some((ref_, _)) = val.raw {
          let mut napi_value = std::ptr::null_mut();
          check_status!(
            unsafe { sys::napi_get_reference_value(env, ref_, &mut napi_value) },
            "Failed to get reference from ArrayBuffer"
          )?;
          check_status!(
            unsafe { sys::napi_delete_reference(env, ref_) },
            "Failed to delete reference in ArrayBuffer::to_napi_value"
          )?;
          val.raw = Some((ptr::null_mut(), ptr::null_mut()));
          return Ok(napi_value);
        }
        let mut arraybuffer_value = ptr::null_mut();
        let ratio = mem::size_of::<$rust_type>();
        let val_length = val.length;
        let length = val_length * ratio;
        let val_data = val.data;
        check_status!(
          if length == 0 {
            // Rust uses 0x1 as the data pointer for empty buffers,
            // but NAPI/V8 only allows multiple buffers to have
            // the same data pointer if it's 0x0.
            unsafe {
              sys::napi_create_arraybuffer(env, length, ptr::null_mut(), &mut arraybuffer_value)
            }
          } else {
            // pull the drop-callback out of `val`
            let fin_cb = val.finalizer_notify.take();

            // Build an independent owner that V8 will hold
            let val_for_js = $name {
              data: val.data,
              length: val.length,
              byte_offset: val.byte_offset,
              raw: None,           // JS copy keeps no napi_ref
              owned_by_rust: true, // it must free the data later
              finalizer_notify: fin_cb,
            };

            // leak the clone to V8, not the original `val`
            let hint_ptr = Box::into_raw(Box::new(val_for_js));
            let status = unsafe {
              sys::napi_create_external_arraybuffer(
                env,
                val_data.cast(),
                length,
                Some(finalizer::<$rust_type, $name>),
                hint_ptr.cast(),
                &mut arraybuffer_value,
              )
            };
            if status == napi_sys::Status::napi_no_external_buffers_allowed {
              let hint = unsafe { Box::from_raw(hint_ptr) };
              let mut underlying_data = ptr::null_mut();
              let status = unsafe {
                sys::napi_create_arraybuffer(
                  env,
                  length,
                  &mut underlying_data,
                  &mut arraybuffer_value,
                )
              };
              unsafe { std::ptr::copy_nonoverlapping(hint.data.cast(), underlying_data, length) };
              status
            } else {
              status
            }
          },
          "Create external arraybuffer failed"
        )?;
        let mut napi_val = ptr::null_mut();
        check_status!(
          unsafe {
            sys::napi_create_typedarray(
              env,
              $typed_array_type as i32,
              val_length,
              arraybuffer_value,
              0,
              &mut napi_val,
            )
          },
          "Create TypedArray failed"
        )?;
        // We handed the buffer to V8 -> drop our callback so we don’t run it
        // twice if Rust side gets dropped first.
        if let Some(cb) = val.take_finalizer() {
          cb(val.data, val.length);
        }
        val.raw = None;
        val.owned_by_rust = false;
        Ok(napi_val)
      }
    }

    impl ToNapiValue for &mut $name {
      unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> Result<sys::napi_value> {
        if let Some((ref_, _)) = val.raw {
          let mut napi_value = std::ptr::null_mut();
          check_status!(
            unsafe { sys::napi_get_reference_value(env, ref_, &mut napi_value) },
            "Failed to get reference from ArrayBuffer"
          )?;
          return Ok(napi_value);
        }
        let mut arraybuffer_value = ptr::null_mut();
        let ratio = mem::size_of::<$rust_type>();
        let val_length = val.length;
        let length = val_length * ratio;
        let val_data = val.data;
        let mut copied_val = None;
        check_status!(
          if length == 0 {
            // Rust uses 0x1 as the data pointer for empty buffers,
            // but NAPI/V8 only allows multiple buffers to have
            // the same data pointer if it's 0x0.
            unsafe {
              sys::napi_create_arraybuffer(env, length, ptr::null_mut(), &mut arraybuffer_value)
            }
          } else {
            // manually copy the data instead of implement `Clone` & `Copy` for TypedArray
            // the TypedArray can't be copied if raw is not None
            let fin_cb = val.finalizer_notify.take();
            let val_copy = $name {
              data: val.data,
              length: val.length,
              byte_offset: val.byte_offset,
              raw: None,
              owned_by_rust: true,
              finalizer_notify: fin_cb,
            };
            let hint_ref: &mut $name = Box::leak(Box::new(val_copy));
            let hint_ptr = hint_ref as *mut $name;
            copied_val = Some(hint_ref);
            let status = unsafe {
              sys::napi_create_external_arraybuffer(
                env,
                val_data.cast(),
                length,
                Some(finalizer::<$rust_type, $name>),
                hint_ptr.cast(),
                &mut arraybuffer_value,
              )
            };
            if status == napi_sys::Status::napi_no_external_buffers_allowed {
              let hint = unsafe { Box::from_raw(hint_ptr) };
              let mut underlying_data = ptr::null_mut();
              let status = unsafe {
                sys::napi_create_arraybuffer(
                  env,
                  length,
                  &mut underlying_data,
                  &mut arraybuffer_value,
                )
              };
              unsafe { std::ptr::copy_nonoverlapping(hint.data.cast(), underlying_data, length) };
              status
            } else {
              status
            }
          },
          "Create external arraybuffer failed"
        )?;
        let mut napi_val = ptr::null_mut();
        check_status!(
          unsafe {
            sys::napi_create_typedarray(
              env,
              $typed_array_type as i32,
              val_length,
              arraybuffer_value,
              0,
              &mut napi_val,
            )
          },
          "Create TypedArray failed"
        )?;

        // Run the drop-callback before we overwrite the pointer/len,
        // otherwise the cb would receive NULL/0.
        if let Some(cb) = val.finalizer_notify.take() {
          cb(val.data, val.length);
        }

        // Give up ownership of the storage that is now held by V8.
        val.raw = None;
        val.owned_by_rust = false;
        val.data = ptr::null_mut();
        val.length = 0;

        // A clone was leaked to V8 (`copied_val`) – make sure that clone
        // no longer tries to free the memory either.
        if let Some(clone) = copied_val {
          clone.raw = None;
        }

        Ok(napi_val)
      }
    }
  };
}

macro_rules! impl_from_slice {
  ($name:ident, $slice_type:ident, $rust_type:ident, $typed_array_type:expr) => {
    #[derive(Clone, Copy)]
    pub struct $slice_type<'env> {
      pub(crate) inner: NonNull<$rust_type>,
      pub(crate) length: usize,
      raw_value: sys::napi_value,
      env: sys::napi_env,
      _marker: PhantomData<&'env ()>,
    }

    impl<'env> $slice_type<'env> {
      #[doc = " Create a new `"]
      #[doc = stringify!($slice_type)]
      #[doc = "` from a `Vec<"]
      #[doc = stringify!($rust_type)]
      #[doc = ">`."]
      pub fn from_data<D: Into<Vec<$rust_type>>>(env: &Env, data: D) -> Result<Self> {
        let mut buf = ptr::null_mut();
        let mut data = data.into();
        let mut inner_ptr = data.as_mut_ptr();

        // element count vs. byte count
        let len_elems = data.len();
        let len_bytes = len_elems * core::mem::size_of::<$rust_type>();

        // Tell V8 how many bytes live outside the JS heap
        let mut _dummy = 0;
        check_status!(
          unsafe { sys::napi_adjust_external_memory(env.0, len_bytes as i64, &mut _dummy) },
          "adjust external memory"
        )?;

        let mut status = unsafe {
          let cap = data.capacity();
          sys::napi_create_external_arraybuffer(
            env.0,
            inner_ptr.cast(),
            len_bytes,
            Some(finalize_slice::<$rust_type>),
            Box::into_raw(Box::new((len_elems, cap))).cast(),
            &mut buf,
          )
        };

        if status == napi_sys::Status::napi_no_external_buffers_allowed {
          let mut inner_data = unsafe { Vec::from_raw_parts(inner_ptr, len_elems, len_elems) };
          let mut underlying_data: *mut c_void = ptr::null_mut();
          status = unsafe {
            sys::napi_create_arraybuffer(env.0, len_bytes, &mut underlying_data, &mut buf)
          };
          unsafe {
            ptr::copy_nonoverlapping(
              inner_data.as_mut_ptr().cast::<u8>(),
              underlying_data.cast::<u8>(),
              len_bytes,
            );
          }
          inner_ptr = underlying_data.cast();
        } else {
          mem::forget(data);
        }
        check_status!(status, "Failed to create buffer slice from data")?;

        #[cfg(all(debug_assertions, not(windows)))]
        if len_elems != 0 {
          register_backing_ptr(inner_ptr.cast::<u8>());
        }

        let mut napi_val = ptr::null_mut();
        check_status!(
          unsafe {
            sys::napi_create_typedarray(
              env.0,
              $typed_array_type as i32,
              len_elems,
              buf,
              0,
              &mut napi_val,
            )
          },
          "Create TypedArray failed"
        )?;

        Ok(Self {
          inner: if len_elems == 0 {
            NonNull::dangling()
          } else {
            unsafe { NonNull::new_unchecked(inner_ptr.cast()) }
          },
          length: len_elems,
          raw_value: napi_val,
          env: env.0,
          _marker: PhantomData,
        })
      }

      pub unsafe fn from_external<T: 'env, F: FnOnce(Env, T)>(
        env: &Env,
        data: *mut u8,
        len: usize,
        finalize_hint: T,
        finalize_callback: F,
      ) -> Result<Self> {
        if data.is_null() || data as *const u8 == crate::EMPTY_VEC.as_ptr() {
          return Err(Error::new(
            Status::InvalidArg,
            "Borrowed data should not be null".to_owned(),
          ));
        }

        let len_bytes = len * core::mem::size_of::<$rust_type>();

        let hint_ptr = Box::into_raw(Box::new((finalize_hint, finalize_callback)));

        let mut arraybuffer_value = ptr::null_mut();
        let mut status = unsafe {
          sys::napi_create_external_arraybuffer(
            env.0,
            data.cast(),
            len_bytes,
            Some(crate::env::raw_finalize_with_custom_callback::<T, F>),
            hint_ptr.cast(),
            &mut arraybuffer_value,
          )
        };

        let mut underlying_data: *mut c_void = ptr::null_mut();
        if status == sys::Status::napi_no_external_buffers_allowed {
          let (hint, finalize) = *Box::from_raw(hint_ptr);
          status = unsafe {
            sys::napi_create_arraybuffer(
              env.0,
              len_bytes,
              &mut underlying_data,
              &mut arraybuffer_value,
            )
          };
          unsafe { ptr::copy_nonoverlapping(data, underlying_data.cast::<u8>(), len_bytes) };
          finalize(*env, hint);
        }

        #[cfg(all(debug_assertions, not(windows)))]
        {
          let ptr_to_track = if status == sys::Status::napi_no_external_buffers_allowed {
            underlying_data // new buffer allocated above
          } else {
            data.cast::<c_void>() // original external buffer
          };
          register_backing_ptr(ptr_to_track.cast::<u8>());
        }

        check_status!(status, "Failed to create arraybuffer from data")?;

        let mut napi_val = ptr::null_mut();
        check_status!(
          unsafe {
            sys::napi_create_typedarray(
              env.0,
              $typed_array_type as i32,
              len,
              arraybuffer_value,
              0,
              &mut napi_val,
            )
          },
          "Create TypedArray failed"
        )?;

        Ok(Self {
          inner: if len == 0 {
            NonNull::dangling()
          } else {
            NonNull::new_unchecked(if status == sys::Status::napi_no_external_buffers_allowed {
              underlying_data.cast()
            } else {
              data.cast()
            })
          },
          length: len,
          raw_value: napi_val,
          env: env.0,
          _marker: PhantomData,
        })
      }
      #[doc = "Copy data from a `&["]
      #[doc = stringify!($rust_type)]
      #[doc = "]` and create a `"]
      #[doc = stringify!($slice_type)]
      #[doc = "` from it."]
      pub fn copy_from<D: AsRef<[$rust_type]>>(env: &Env, data: D) -> Result<Self> {
        let data = data.as_ref();
        let len = data.len();
        let mut arraybuffer_value = ptr::null_mut();
        let mut underlying_data = ptr::null_mut();

        check_status!(
          unsafe {
            sys::napi_create_arraybuffer(env.0, len, &mut underlying_data, &mut arraybuffer_value)
          },
          "Failed to create ArrayBuffer"
        )?;

        let mut napi_val = ptr::null_mut();
        check_status!(
          unsafe {
            sys::napi_create_typedarray(
              env.0,
              $typed_array_type as i32,
              len,
              arraybuffer_value,
              0,
              &mut napi_val,
            )
          },
          "Create TypedArray failed"
        )?;

        Ok(Self {
          inner: if len == 0 {
            NonNull::dangling()
          } else {
            unsafe { NonNull::new_unchecked(underlying_data.cast()) }
          },
          length: len,
          raw_value: napi_val,
          env: env.0,
          _marker: PhantomData,
        })
      }

      /// Create from `ArrayBuffer`
      pub fn from_arraybuffer(
        arraybuffer: &ArrayBuffer<'env>,
        byte_offset: usize,
        length: usize,
      ) -> Result<$slice_type<'env>> {
        let env = arraybuffer.value.env;
        let mut typed_array = ptr::null_mut();
        check_status!(
          unsafe {
            sys::napi_create_typedarray(
              env,
              $typed_array_type.into(),
              length,
              arraybuffer.value().value,
              byte_offset,
              &mut typed_array,
            )
          },
          "Failed to create TypedArray from ArrayBuffer"
        )?;

        unsafe { FromNapiValue::from_napi_value(env, typed_array) }
      }

      /// extends the lifetime of the `TypedArray` to the lifetime of the `This`
      pub fn assign_to_this<'a, U>(&self, this: This<'a, U>, name: &str) -> Result<$slice_type<'a>>
      where
        U: FromNapiValue + JsObjectValue<'a>,
      {
        let name = CString::new(name)?;
        check_status!(
          unsafe {
            sys::napi_set_named_property(self.env, this.object.raw(), name.as_ptr(), self.raw_value)
          },
          "Failed to assign {} to this",
          $slice_type::type_name()
        )?;
        Ok($slice_type {
          env: self.env,
          raw_value: self.raw_value,
          inner: self.inner,
          length: self.length,
          _marker: PhantomData,
        })
      }

      #[doc = "Convert a `"]
      #[doc = stringify!($slice_type)]
      #[doc = "` to a `"]
      #[doc = stringify!($name)]
      #[doc = "`."]
      #[doc = ""]
      #[doc = "This will perform a `napi_create_reference` internally."]
      pub fn into_typed_array(self, env: &Env) -> Result<$name> {
        unsafe { $name::from_napi_value(env.0, self.raw_value) }
      }
    }

    impl<'env> JsValue<'env> for $slice_type<'env> {
      fn value(&self) -> Value {
        Value {
          env: self.env,
          value: self.raw_value,
          value_type: ValueType::Object,
        }
      }
    }

    impl<'env> JsObjectValue<'env> for $slice_type<'env> {}

    impl ToNapiValue for &$slice_type<'_> {
      unsafe fn to_napi_value(_: sys::napi_env, val: Self) -> Result<sys::napi_value> {
        Ok(val.raw_value)
      }
    }

    impl ToNapiValue for &mut $slice_type<'_> {
      unsafe fn to_napi_value(_: sys::napi_env, val: Self) -> Result<sys::napi_value> {
        Ok(val.raw_value)
      }
    }

    impl FromNapiValue for $slice_type<'_> {
      unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
        let mut typed_array_type = 0;
        let mut length = 0;
        let mut data = ptr::null_mut();
        let mut array_buffer = ptr::null_mut();
        let mut byte_offset = 0;
        check_status!(
          unsafe {
            sys::napi_get_typedarray_info(
              env,
              napi_val,
              &mut typed_array_type,
              &mut length,
              &mut data,
              &mut array_buffer,
              &mut byte_offset,
            )
          },
          "Get TypedArray info failed"
        )?;
        if typed_array_type != $typed_array_type as i32 {
          return Err(Error::new(
            Status::InvalidArg,
            format!("Expected $name, got {}", typed_array_type),
          ));
        }
        // From the docs of `napi_get_typedarray_info`:
        // > [out] data: The underlying data buffer of the node::Buffer. If length is 0, this may be
        // > NULL or any other pointer value.
        //
        // In order to guarantee that `slice::from_raw_parts` is sound, the pointer must be non-null, so
        // let's make sure it always is, even in the case of `napi_get_typedarray_info` returning a null
        // ptr.
        Ok(Self {
          inner: if length == 0 {
            ptr::NonNull::dangling()
          } else {
            ptr::NonNull::new_unchecked(data.cast())
          },
          length,
          raw_value: napi_val,
          env,
          _marker: PhantomData,
        })
      }
    }

    impl TypeName for $slice_type<'_> {
      fn type_name() -> &'static str {
        concat!("TypedArray<", stringify!($rust_type), ">")
      }

      fn value_type() -> crate::ValueType {
        crate::ValueType::Object
      }
    }

    impl ValidateNapiValue for $slice_type<'_> {
      unsafe fn validate(env: sys::napi_env, napi_val: sys::napi_value) -> Result<sys::napi_value> {
        let mut is_typed_array = false;
        check_status!(
          unsafe { sys::napi_is_typedarray(env, napi_val, &mut is_typed_array) },
          "Failed to validate napi typed array"
        )?;
        if !is_typed_array {
          return Err(Error::new(
            Status::InvalidArg,
            "Expected a TypedArray value".to_owned(),
          ));
        }
        Ok(ptr::null_mut())
      }
    }

    impl AsRef<[$rust_type]> for $slice_type<'_> {
      fn as_ref(&self) -> &[$rust_type] {
        unsafe { core::slice::from_raw_parts(self.inner.as_ptr(), self.length) }
      }
    }

    impl Deref for $slice_type<'_> {
      type Target = [$rust_type];

      fn deref(&self) -> &Self::Target {
        self.as_ref()
      }
    }

    impl DerefMut for $slice_type<'_> {
      fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { core::slice::from_raw_parts_mut(self.inner.as_ptr(), self.length) }
      }
    }

    impl FromNapiValue for &mut [$rust_type] {
      unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
        let mut typed_array_type = 0;
        let mut length = 0;
        let mut data = ptr::null_mut();
        let mut array_buffer = ptr::null_mut();
        let mut byte_offset = 0;
        check_status!(
          unsafe {
            sys::napi_get_typedarray_info(
              env,
              napi_val,
              &mut typed_array_type,
              &mut length,
              &mut data,
              &mut array_buffer,
              &mut byte_offset,
            )
          },
          "Get TypedArray info failed"
        )?;
        if typed_array_type != $typed_array_type as i32 {
          return Err(Error::new(
            Status::InvalidArg,
            format!("Expected $name, got {}", typed_array_type),
          ));
        }
        Ok(if length == 0 {
          &mut []
        } else {
          unsafe { core::slice::from_raw_parts_mut(data as *mut $rust_type, length) }
        })
      }
    }

    impl FromNapiValue for &[$rust_type] {
      unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
        let mut typed_array_type = 0;
        let mut length = 0;
        let mut data = ptr::null_mut();
        let mut array_buffer = ptr::null_mut();
        let mut byte_offset = 0;
        check_status!(
          unsafe {
            sys::napi_get_typedarray_info(
              env,
              napi_val,
              &mut typed_array_type,
              &mut length,
              &mut data,
              &mut array_buffer,
              &mut byte_offset,
            )
          },
          "Get TypedArray info failed"
        )?;
        if typed_array_type != $typed_array_type as i32 {
          return Err(Error::new(
            Status::InvalidArg,
            format!("Expected $name, got {}", typed_array_type),
          ));
        }
        Ok(if length == 0 {
          &[]
        } else {
          unsafe { core::slice::from_raw_parts_mut(data as *mut $rust_type, length) }
        })
      }
    }

    impl TypeName for &mut [$rust_type] {
      fn type_name() -> &'static str {
        concat!("TypedArray<", stringify!($rust_type), ">")
      }

      fn value_type() -> crate::ValueType {
        crate::ValueType::Object
      }
    }

    impl TypeName for &[$rust_type] {
      fn type_name() -> &'static str {
        concat!("TypedArray<", stringify!($rust_type), ">")
      }

      fn value_type() -> crate::ValueType {
        crate::ValueType::Object
      }
    }

    impl ValidateNapiValue for &[$rust_type] {
      unsafe fn validate(env: sys::napi_env, napi_val: sys::napi_value) -> Result<sys::napi_value> {
        let mut is_typed_array = false;
        check_status!(
          unsafe { sys::napi_is_typedarray(env, napi_val, &mut is_typed_array) },
          "Failed to validate napi typed array"
        )?;
        if !is_typed_array {
          return Err(Error::new(
            Status::InvalidArg,
            "Expected a TypedArray value".to_owned(),
          ));
        }
        Ok(ptr::null_mut())
      }
    }

    impl ValidateNapiValue for &mut [$rust_type] {
      unsafe fn validate(env: sys::napi_env, napi_val: sys::napi_value) -> Result<sys::napi_value> {
        let mut is_typed_array = false;
        check_status!(
          unsafe { sys::napi_is_typedarray(env, napi_val, &mut is_typed_array) },
          "Failed to validate napi typed array"
        )?;
        if !is_typed_array {
          return Err(Error::new(
            Status::InvalidArg,
            "Expected a TypedArray value".to_owned(),
          ));
        }
        Ok(ptr::null_mut())
      }
    }
  };
}

unsafe extern "C" fn finalizer<Data, T: Finalizer<RustType = Data>>(
  env: sys::napi_env,
  _finalize_data: *mut c_void,
  finalize_hint: *mut c_void,
) {
  crate::bindgen_runtime::IN_FINALISER.with(|f| f.set(true));

  let data: T = *Box::from_raw(finalize_hint.cast::<T>());

  if !env.is_null() {
    let mut _dummy = 0;
    // tell V8 the bytes are gone
    sys::napi_adjust_external_memory(env, -(data.byte_len() as i64), &mut _dummy);
  }
  // now drop them
  drop(data);
  crate::bindgen_runtime::IN_FINALISER.with(|f| f.set(false));
}

unsafe extern "C" fn finalize_slice<Data>(
  _env: sys::napi_env,
  finalize_data: *mut c_void,
  finalize_hint: *mut c_void,
) {
  #[cfg(all(debug_assertions, not(windows)))]
  unregister_backing_ptr(finalize_data as *mut u8);

  let (length, cap) = *Box::from_raw(finalize_hint.cast::<(usize, usize)>());
  Vec::from_raw_parts(finalize_data.cast::<Data>(), length, cap);

  // balance external-memory counter
  let mut _dummy = 0;
  sys::napi_adjust_external_memory(_env, -(length as i64), &mut _dummy);
}

impl_typed_array!(Int8Array, i8, TypedArrayType::Int8);
impl_from_slice!(Int8Array, Int8ArraySlice, i8, TypedArrayType::Int8);
impl_typed_array!(Uint8Array, u8, TypedArrayType::Uint8);
impl_from_slice!(Uint8Array, Uint8ArraySlice, u8, TypedArrayType::Uint8);
impl_typed_array!(Uint8ClampedArray, u8, TypedArrayType::Uint8Clamped);
impl_typed_array!(Int16Array, i16, TypedArrayType::Int16);
impl_from_slice!(Int16Array, Int16ArraySlice, i16, TypedArrayType::Int16);
impl_typed_array!(Uint16Array, u16, TypedArrayType::Uint16);
impl_from_slice!(Uint16Array, Uint16ArraySlice, u16, TypedArrayType::Uint16);
impl_typed_array!(Int32Array, i32, TypedArrayType::Int32);
impl_from_slice!(Int32Array, Int32ArraySlice, i32, TypedArrayType::Int32);
impl_typed_array!(Uint32Array, u32, TypedArrayType::Uint32);
impl_from_slice!(Uint32Array, Uint32ArraySlice, u32, TypedArrayType::Uint32);
impl_typed_array!(Float32Array, f32, TypedArrayType::Float32);
impl_from_slice!(
  Float32Array,
  Float32ArraySlice,
  f32,
  TypedArrayType::Float32
);
impl_typed_array!(Float64Array, f64, TypedArrayType::Float64);
impl_from_slice!(
  Float64Array,
  Float64ArraySlice,
  f64,
  TypedArrayType::Float64
);
#[cfg(feature = "napi6")]
impl_typed_array!(BigInt64Array, i64, TypedArrayType::BigInt64);
#[cfg(feature = "napi6")]
impl_from_slice!(
  BigInt64Array,
  BigInt64ArraySlice,
  i64,
  TypedArrayType::BigInt64
);
#[cfg(feature = "napi6")]
impl_typed_array!(BigUint64Array, u64, TypedArrayType::BigUint64);
#[cfg(feature = "napi6")]
impl_from_slice!(
  BigUint64Array,
  BigUint64ArraySlice,
  u64,
  TypedArrayType::BigUint64
);

impl Uint8Array {
  /// Create a new JavaScript `Uint8Array` from a Rust `String` without copying the underlying data.
  pub fn from_string(mut s: String) -> Self {
    let len = s.len();
    let cap = s.capacity();

    let ret = Self {
      data: s.as_mut_ptr(),
      length: len,
      owned_by_rust: true,
      finalizer_notify: Some(Box::new(move |data, _| {
        // Re-create the String so Rust will free it.
        drop(unsafe { String::from_raw_parts(data, len, cap) });
      })),
      byte_offset: 0,
      raw: None,
    };

    // Prevent Rust from freeing the String now – JS owns it.
    mem::forget(s);
    ret
  }
}

#[derive(Clone, Copy)]
/// Zero copy Uint8ClampedArray slice shared between Rust and Node.js.
/// It can only be used in non-async context and the lifetime is bound to the fn closure.
/// If you want to use Node.js `Uint8ClampedArray` in async context or want to extend the lifetime, use `Uint8ClampedArray` instead.
pub struct Uint8ClampedSlice<'scope> {
  pub(crate) inner: NonNull<u8>,
  pub(crate) length: usize,
  raw_value: sys::napi_value,
  env: sys::napi_env,
  _marker: PhantomData<&'scope ()>,
}

impl FromNapiValue for Uint8ClampedSlice<'_> {
  unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
    let mut typed_array_type = 0;
    let mut length = 0;
    let mut data = ptr::null_mut();
    let mut array_buffer = ptr::null_mut();
    let mut byte_offset = 0;
    check_status!(
      unsafe {
        sys::napi_get_typedarray_info(
          env,
          napi_val,
          &mut typed_array_type,
          &mut length,
          &mut data,
          &mut array_buffer,
          &mut byte_offset,
        )
      },
      "Get TypedArray info failed"
    )?;
    if typed_array_type != TypedArrayType::Uint8Clamped as i32 {
      return Err(Error::new(
        Status::InvalidArg,
        format!("Expected $name, got {}", typed_array_type),
      ));
    }
    Ok(Self {
      inner: if length == 0 {
        NonNull::dangling()
      } else {
        unsafe { NonNull::new_unchecked(data.cast()) }
      },
      length,
      raw_value: napi_val,
      env,
      _marker: PhantomData,
    })
  }
}

impl<'env> JsValue<'env> for Uint8ClampedSlice<'env> {
  fn value(&self) -> Value {
    Value {
      env: self.env,
      value: self.raw_value,
      value_type: ValueType::Object,
    }
  }
}

impl<'env> JsObjectValue<'env> for Uint8ClampedSlice<'env> {}

impl TypeName for Uint8ClampedSlice<'_> {
  fn type_name() -> &'static str {
    "Uint8ClampedArray"
  }

  fn value_type() -> ValueType {
    ValueType::Object
  }
}

impl ValidateNapiValue for Uint8ClampedSlice<'_> {
  unsafe fn validate(env: sys::napi_env, napi_val: sys::napi_value) -> Result<sys::napi_value> {
    let mut is_typedarray = false;
    check_status!(
      unsafe { sys::napi_is_typedarray(env, napi_val, &mut is_typedarray) },
      "Failed to validate typed buffer"
    )?;
    if !is_typedarray {
      return Err(Error::new(
        Status::InvalidArg,
        "Expected a TypedArray value".to_owned(),
      ));
    }
    Ok(ptr::null_mut())
  }
}

impl AsRef<[u8]> for Uint8ClampedSlice<'_> {
  fn as_ref(&self) -> &[u8] {
    unsafe { core::slice::from_raw_parts(self.inner.as_ptr(), self.length) }
  }
}

impl Deref for Uint8ClampedSlice<'_> {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    unsafe { core::slice::from_raw_parts(self.inner.as_ptr(), self.length) }
  }
}

impl DerefMut for Uint8ClampedSlice<'_> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe { core::slice::from_raw_parts_mut(self.inner.as_ptr(), self.length) }
  }
}

impl<'env> Uint8ClampedSlice<'env> {
  /// Create a new `Uint8ClampedSlice` from Vec<u8>
  pub fn from_data<D: Into<Vec<u8>>>(env: &Env, data: D) -> Result<Self> {
    let mut buf = ptr::null_mut();
    let mut data = data.into();
    let mut inner_ptr = data.as_mut_ptr();
    let len = data.len();

    // tell V8 how many bytes live outside the JS heap
    let mut _dummy = 0;
    check_status!(
      unsafe { sys::napi_adjust_external_memory(env.0, len as i64, &mut _dummy) },
      "adjust external memory"
    )?;

    let mut status = unsafe {
      let cap = data.capacity();
      sys::napi_create_external_arraybuffer(
        env.0,
        inner_ptr.cast(),
        len,
        Some(finalize_slice::<u8>),
        Box::into_raw(Box::new((len, cap))).cast(),
        &mut buf,
      )
    };

    if status == napi_sys::Status::napi_no_external_buffers_allowed {
      // Fallback: allocate a fresh ArrayBuffer
      let mut inner_data = unsafe { Vec::from_raw_parts(inner_ptr, len, len) };
      let mut underlying_data = ptr::null_mut();
      status = unsafe { sys::napi_create_arraybuffer(env.0, len, &mut underlying_data, &mut buf) };
      unsafe {
        std::ptr::copy_nonoverlapping(inner_data.as_mut_ptr().cast(), underlying_data, len);
      }
      inner_ptr = underlying_data.cast();
    } else {
      mem::forget(data);
    }
    check_status!(status, "Failed to create buffer slice from data")?;

    #[cfg(all(debug_assertions, not(windows)))]
    {
      register_backing_ptr(inner_ptr); // inner_ptr is final backing store
    }

    // create TypedArray
    let mut napi_val = ptr::null_mut();
    check_status!(
      unsafe {
        sys::napi_create_typedarray(
          env.0,
          TypedArrayType::Uint8Clamped as i32,
          len,
          buf,
          0,
          &mut napi_val,
        )
      },
      "Create TypedArray failed"
    )?;

    Ok(Self {
      inner: if len == 0 {
        NonNull::dangling()
      } else {
        unsafe { NonNull::new_unchecked(inner_ptr.cast()) }
      },
      length: len,
      raw_value: napi_val,
      env: env.0,
      _marker: PhantomData,
    })
  }

  /// ## Safety
  ///
  /// Mostly the same with `from_data`
  ///
  /// Provided `finalize_callback` will be called when `Uint8ClampedSlice` got dropped.
  ///
  /// You can pass in `noop_finalize` if you have nothing to do in finalize phase.
  ///
  /// ### Notes
  ///
  /// JavaScript may mutate the data passed in to this buffer when writing the buffer.
  ///
  /// However, some JavaScript runtimes do not support external buffers (notably electron!)
  ///
  /// in which case modifications may be lost.
  ///
  /// If you need to support these runtimes, you should create a buffer by other means and then
  /// later copy the data back out.
  pub unsafe fn from_external<T: 'env, F: FnOnce(Env, T)>(
    env: &Env,
    data: *mut u8,
    len: usize,
    finalize_hint: T,
    finalize_callback: F,
  ) -> Result<Self> {
    if data.is_null() || std::ptr::eq(data, crate::EMPTY_VEC.as_ptr()) {
      return Err(Error::new(
        Status::InvalidArg,
        "Borrowed data should not be null".to_owned(),
      ));
    }
    #[cfg(all(debug_assertions, not(windows)))]
    register_backing_ptr(data);
    let hint_ptr = Box::into_raw(Box::new((finalize_hint, finalize_callback)));
    let mut arraybuffer_value = ptr::null_mut();
    let mut status = unsafe {
      sys::napi_create_external_arraybuffer(
        env.0,
        data.cast(),
        len,
        Some(crate::env::raw_finalize_with_custom_callback::<T, F>),
        hint_ptr.cast(),
        &mut arraybuffer_value,
      )
    };
    status = if status == sys::Status::napi_no_external_buffers_allowed {
      let (hint, finalize) = *Box::from_raw(hint_ptr);
      let mut underlying_data = ptr::null_mut();
      let status = unsafe {
        sys::napi_create_arraybuffer(env.0, len, &mut underlying_data, &mut arraybuffer_value)
      };
      unsafe { std::ptr::copy_nonoverlapping(data.cast(), underlying_data, len) };
      finalize(*env, hint);
      status
    } else {
      status
    };
    check_status!(status, "Failed to create arraybuffer from data")?;

    let mut napi_val = ptr::null_mut();
    check_status!(
      unsafe {
        sys::napi_create_typedarray(
          env.0,
          TypedArrayType::Uint8Clamped as i32,
          len,
          arraybuffer_value,
          0,
          &mut napi_val,
        )
      },
      "Create TypedArray failed"
    )?;

    Ok(Self {
      inner: if len == 0 {
        NonNull::dangling()
      } else {
        unsafe { NonNull::new_unchecked(data.cast()) }
      },
      length: len,
      raw_value: napi_val,
      env: env.0,
      _marker: PhantomData,
    })
  }

  /// Copy data from a `&[u8]` and create a `Uint8ClampedSlice` from it.
  pub fn copy_from<D: AsRef<[u8]>>(env: &Env, data: D) -> Result<Self> {
    let data = data.as_ref();
    let len = data.len();
    let mut arraybuffer_value = ptr::null_mut();
    let mut underlying_data = ptr::null_mut();

    check_status!(
      unsafe {
        sys::napi_create_arraybuffer(env.0, len, &mut underlying_data, &mut arraybuffer_value)
      },
      "Failed to create ArrayBuffer"
    )?;

    let mut napi_val = ptr::null_mut();
    check_status!(
      unsafe {
        sys::napi_create_typedarray(
          env.0,
          TypedArrayType::Uint8Clamped as i32,
          len,
          arraybuffer_value,
          0,
          &mut napi_val,
        )
      },
      "Create TypedArray failed"
    )?;

    Ok(Self {
      inner: if len == 0 {
        NonNull::dangling()
      } else {
        unsafe { NonNull::new_unchecked(underlying_data.cast()) }
      },
      length: len,
      raw_value: napi_val,
      env: env.0,
      _marker: PhantomData,
    })
  }

  /// Create from `ArrayBuffer`
  pub fn from_arraybuffer(
    arraybuffer: &ArrayBuffer<'env>,
    byte_offset: usize,
    length: usize,
  ) -> Result<Self> {
    let env = arraybuffer.value.env;
    let mut typed_array = ptr::null_mut();
    check_status!(
      unsafe {
        sys::napi_create_typedarray(
          env,
          TypedArrayType::Uint8Clamped as i32,
          length,
          arraybuffer.value().value,
          byte_offset,
          &mut typed_array,
        )
      },
      "Failed to create TypedArray from ArrayBuffer"
    )?;

    unsafe { FromNapiValue::from_napi_value(env, typed_array) }
  }

  /// extends the lifetime of the `TypedArray` to the lifetime of the `This`
  pub fn assign_to_this<'a, U>(&self, this: This<'a, U>, name: &str) -> Result<Self>
  where
    U: FromNapiValue + JsObjectValue<'a>,
  {
    let name = CString::new(name)?;
    check_status!(
      unsafe {
        sys::napi_set_named_property(self.env, this.object.raw(), name.as_ptr(), self.raw_value)
      },
      "Failed to assign {} to this",
      Self::type_name()
    )?;
    Ok(Self {
      env: self.env,
      raw_value: self.raw_value,
      inner: self.inner,
      length: self.length,
      _marker: PhantomData,
    })
  }

  /// Convert a `Uint8ClampedSlice` to a `Uint8ClampedArray`.
  pub fn into_typed_array(self, env: &Env) -> Result<Self> {
    unsafe { Self::from_napi_value(env.0, self.raw_value) }
  }
}

impl<T: Into<Vec<u8>>> From<T> for Uint8Array {
  fn from(data: T) -> Self {
    Uint8Array::new(data.into())
  }
}

impl<T: Into<Vec<u8>>> From<T> for Uint8ClampedArray {
  fn from(data: T) -> Self {
    Uint8ClampedArray::new(data.into())
  }
}

impl<T: Into<Vec<u16>>> From<T> for Uint16Array {
  fn from(data: T) -> Self {
    Uint16Array::new(data.into())
  }
}

impl<T: Into<Vec<u32>>> From<T> for Uint32Array {
  fn from(data: T) -> Self {
    Uint32Array::new(data.into())
  }
}

impl<T: Into<Vec<i8>>> From<T> for Int8Array {
  fn from(data: T) -> Self {
    Int8Array::new(data.into())
  }
}

impl<T: Into<Vec<i16>>> From<T> for Int16Array {
  fn from(data: T) -> Self {
    Int16Array::new(data.into())
  }
}

impl<T: Into<Vec<i32>>> From<T> for Int32Array {
  fn from(data: T) -> Self {
    Int32Array::new(data.into())
  }
}

impl<T: Into<Vec<f32>>> From<T> for Float32Array {
  fn from(data: T) -> Self {
    Float32Array::new(data.into())
  }
}

impl<T: Into<Vec<f64>>> From<T> for Float64Array {
  fn from(data: T) -> Self {
    Float64Array::new(data.into())
  }
}

#[cfg(feature = "napi6")]
impl<T: Into<Vec<i64>>> From<T> for BigInt64Array {
  fn from(data: T) -> Self {
    BigInt64Array::new(data.into())
  }
}
#[cfg(feature = "napi6")]
impl<T: Into<Vec<u64>>> From<T> for BigUint64Array {
  fn from(data: T) -> Self {
    BigUint64Array::new(data.into())
  }
}
