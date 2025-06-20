#[cfg(all(debug_assertions, not(windows)))]
use std::collections::HashSet;
use std::ffi::c_void;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr::{self, NonNull};
use std::slice;
#[cfg(all(debug_assertions, not(windows)))]
use std::sync::Mutex;

use crate::{
  bindgen_prelude::*, check_status, env::EMPTY_VEC, sys, JsValue, Result, Value, ValueType,
};

#[cfg(all(debug_assertions, not(windows)))]
thread_local! {
  pub (crate) static BUFFER_DATA: Mutex<HashSet<*mut u8>> = Default::default();
}

#[cfg(all(debug_assertions, not(windows)))]
#[inline]
pub fn register_backing_ptr(ptr: *mut u8) {
  if ptr.is_null() {
    return;
  } // 0-length buffers use NULL
  BUFFER_DATA.with(|buffer_data| {
    let mut set = buffer_data.lock().unwrap();
    if !set.insert(ptr) {
      panic!(
        "Share the same data between different buffers is not allowed, \
                    see: https://github.com/nodejs/node/issues/32463#issuecomment-631974747"
      );
    }
  });
}

#[cfg(all(debug_assertions, not(windows)))]
#[inline]
pub fn unregister_backing_ptr(ptr: *mut u8) {
  if ptr.is_null() {
    return;
  }
  BUFFER_DATA.with(|buffer_data| {
    let mut set = buffer_data.lock().unwrap();
    set.remove(&ptr);
  });
}

#[cfg(feature = "trace-buffer-drops")]
macro_rules! trace_drop {
    ($($arg:tt)*) => {
        eprintln!("[buffer-drop] {}", format_args!($($arg)*));
    };
}

#[cfg(not(feature = "trace-buffer-drops"))]
macro_rules! trace_drop {
  ($($arg:tt)*) => {};
}

/// Zero copy buffer slice shared between Rust and Node.js.
///
/// It can only be used in non-async context and the lifetime is bound to the fn closure.
///
/// If you want to use Node.js Buffer in async context or want to extend the lifetime, use `Buffer` instead.
pub struct BufferSlice<'env> {
  pub(crate) inner: &'env mut [u8],
  pub(crate) raw_value: sys::napi_value,
  #[allow(dead_code)]
  pub(crate) env: sys::napi_env,
}

impl<'env> BufferSlice<'env> {
  /// Create a new `BufferSlice` from a `Vec<u8>`.
  ///
  /// While this is still a fully-supported data structure, in most cases using a `Uint8Array` will suffice.
  pub fn from_data<D: Into<Vec<u8>>>(env: &Env, data: D) -> Result<Self> {
    let mut js_value = ptr::null_mut();
    let mut vec = data.into();
    let len = vec.len();

    let mut _dummy = 0;
    check_status!(
      unsafe { sys::napi_adjust_external_memory(env.0, len as i64, &mut _dummy) },
      "adjust external memory"
    )?;

    if len == 0 {
      check_status!(
        unsafe { sys::napi_create_buffer(env.0, 0, ptr::null_mut(), &mut js_value) },
        "Failed to create zero-length BufferSlice"
      )?;
      mem::forget(vec);
      return Ok(Self {
        inner: &mut [],
        raw_value: js_value,
        env: env.0,
      });
    }

    let src_ptr = vec.as_mut_ptr();
    let mut status = unsafe {
      sys::napi_create_external_buffer(
        env.0,
        len,
        src_ptr.cast(),
        Some(drop_buffer_slice),
        Box::into_raw(Box::new((len, vec.capacity()))).cast(),
        &mut js_value,
      )
    };

    let mut backing_ptr: *mut c_void = src_ptr.cast();

    if status == sys::Status::napi_no_external_buffers_allowed {
      let mut copy_ptr: *mut c_void = ptr::null_mut();
      status = unsafe {
        sys::napi_create_buffer_copy(env.0, len, src_ptr.cast(), &mut copy_ptr, &mut js_value)
      };
      backing_ptr = copy_ptr;
    }

    mem::forget(vec);
    check_status!(status, "Failed to create BufferSlice")?;

    #[cfg(all(debug_assertions, not(windows)))]
    register_backing_ptr(backing_ptr.cast::<u8>());

    Ok(Self {
      inner: unsafe { slice::from_raw_parts_mut(backing_ptr.cast::<u8>(), len) },
      raw_value: js_value,
      env: env.0,
    })
  }

  /// ## Safety
  ///
  /// Mostly the same with `from_data`
  ///
  /// Provided `finalize_callback` will be called when `BufferSlice` got dropped.
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
    if data.is_null() || std::ptr::eq(data, EMPTY_VEC.as_ptr()) {
      return Err(Error::new(
        Status::InvalidArg,
        "Borrowed data should not be null".to_owned(),
      ));
    }

    // fast path: external buffer
    let mut buf = ptr::null_mut();
    let hint_ptr = Box::into_raw(Box::new((finalize_hint, finalize_callback)));
    let mut status = sys::napi_create_external_buffer(
      env.0,
      len,
      data.cast(),
      Some(crate::env::raw_finalize_with_custom_callback::<T, F>),
      hint_ptr.cast(),
      &mut buf,
    );

    let mut copied_ptr: *mut c_void = ptr::null_mut();

    if status == sys::Status::napi_no_external_buffers_allowed {
      let (hint, finalize) = *Box::from_raw(hint_ptr);
      status = sys::napi_create_buffer_copy(env.0, len, data.cast(), &mut copied_ptr, &mut buf);
      finalize(*env, hint);
    }

    check_status!(status, "Failed to create buffer slice from data")?;

    #[cfg(all(debug_assertions, not(windows)))]
    {
      let ptr_to_track: *mut u8 = if status == sys::Status::napi_no_external_buffers_allowed {
        copied_ptr.cast() // new backing store from buffer-copy
      } else {
        data // original external buffer pointer
      };
      register_backing_ptr(ptr_to_track);
    }

    Ok(Self {
      inner: if len == 0 {
        &mut []
      } else {
        slice::from_raw_parts_mut(buf.cast(), len)
      },
      raw_value: buf,
      env: env.0,
    })
  }

  /// Copy data from a `&[u8]` and create a `BufferSlice` from it.
  pub fn copy_from<D: AsRef<[u8]>>(env: &Env, data: D) -> Result<Self> {
    let bytes = data.as_ref();
    let len = bytes.len();

    // `result_ptr` will be the real backing-store pointer V8 owns.
    let mut js_value = ptr::null_mut(); // the Buffer object returned
    let mut result_ptr = ptr::null_mut(); // raw memory inside that Buffer
    check_status!(
      unsafe {
        sys::napi_create_buffer_copy(
          env.0,
          len,
          bytes.as_ptr().cast(),
          &mut result_ptr,
          &mut js_value,
        )
      },
      "Failed to create BufferSlice from copied data"
    )?;

    let slice: &mut [u8] = if len == 0 {
      &mut []
    } else {
      // SAFETY: `result_ptr` points to the freshly-allocated Buffer memory
      unsafe { slice::from_raw_parts_mut(result_ptr.cast::<u8>(), len) }
    };

    Ok(Self {
      inner: slice,
      raw_value: js_value,
      env: env.0,
    })
  }

  /// Convert a `BufferSlice` to a `Buffer`
  ///
  /// This will perform a `napi_create_reference` internally.
  pub fn into_buffer(self, env: &Env) -> Result<Buffer> {
    unsafe { Buffer::from_napi_value(env.0, self.raw_value) }
  }
}

impl<'env> JsValue<'env> for BufferSlice<'env> {
  fn value(&self) -> Value {
    Value {
      env: self.env,
      value: self.raw_value,
      value_type: ValueType::Object,
    }
  }
}

impl<'env> JsObjectValue<'env> for BufferSlice<'env> {}

impl FromNapiValue for BufferSlice<'_> {
  unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
    let mut buf = ptr::null_mut();
    let mut len = 0usize;
    check_status!(
      unsafe { sys::napi_get_buffer_info(env, napi_val, &mut buf, &mut len) },
      "Failed to get Buffer pointer and length"
    )?;
    // From the docs of `napi_get_buffer_info`:
    // > [out] data: The underlying data buffer of the node::Buffer. If length is 0, this may be
    // > NULL or any other pointer value.
    //
    // In order to guarantee that `slice::from_raw_parts` is sound, the pointer must be non-null, so
    // let's make sure it always is, even in the case of `napi_get_buffer_info` returning a null
    // ptr.
    Ok(Self {
      inner: if len == 0 {
        &mut []
      } else {
        unsafe { slice::from_raw_parts_mut(buf.cast(), len) }
      },
      raw_value: napi_val,
      env,
    })
  }
}

impl ToNapiValue for &BufferSlice<'_> {
  #[allow(unused_variables)]
  unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> Result<sys::napi_value> {
    Ok(val.raw_value)
  }
}

impl TypeName for BufferSlice<'_> {
  fn type_name() -> &'static str {
    "Buffer"
  }

  fn value_type() -> ValueType {
    ValueType::Object
  }
}

impl ValidateNapiValue for BufferSlice<'_> {
  unsafe fn validate(env: sys::napi_env, napi_val: sys::napi_value) -> Result<sys::napi_value> {
    let mut is_buffer = false;
    check_status!(
      unsafe { sys::napi_is_buffer(env, napi_val, &mut is_buffer) },
      "Failed to validate napi buffer"
    )?;
    if !is_buffer {
      return Err(Error::new(
        Status::InvalidArg,
        "Expected a Buffer value".to_owned(),
      ));
    }
    Ok(ptr::null_mut())
  }
}

impl AsRef<[u8]> for BufferSlice<'_> {
  fn as_ref(&self) -> &[u8] {
    self.inner
  }
}

impl Deref for BufferSlice<'_> {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    self.inner
  }
}

impl DerefMut for BufferSlice<'_> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.inner
  }
}

/// Zero copy u8 vector shared between rust and napi.
/// It's designed to be used in `async` context, so it contains overhead to ensure the underlying data is not dropped.
/// For non-async context, use `BufferRef` instead.
///
/// Auto reference the raw JavaScript value, and release it when dropped.
/// So it is safe to use it in `async fn`, the `&[u8]` under the hood will not be dropped until the `drop` called.
/// Clone will create a new `Reference` to the same underlying `JavaScript Buffer`.
pub struct Buffer {
  pub(crate) inner: NonNull<u8>,
  pub(crate) len: usize,
  pub(crate) capacity: usize,
  raw: Option<(sys::napi_ref, sys::napi_env)>,
  owned_by_rust: bool,
}

impl Drop for Buffer {
  fn drop(&mut self) {
    // Fast-path: Buffer originated from Vec<u8>
    if self.raw.is_none() && self.owned_by_rust {
      trace_drop!("from-vec len {}", self.len);
      unsafe {
        Vec::from_raw_parts(self.inner.as_ptr(), self.len, self.capacity);
      }
      return;
    }

    let (ref_, env) = self.raw.unwrap();
    if ref_.is_null() {
      return;
    }

    let mut ref_count = 0;
    check_status_or_throw!(
      env,
      unsafe { sys::napi_reference_unref(env, ref_, &mut ref_count) },
      "Failed to unref Buffer reference in drop",
    );
    trace_drop!("unref → {}  (ptr = {:?})", ref_count, ref_);

    if ref_count != 0 {
      // There are still JS/Rust handles alive; leave reference in place.
      return;
    }

    // N-API 9+
    #[cfg(all(feature = "napi9", not(feature = "noop")))]
    unsafe {
      unsafe extern "C" fn do_delete(env: sys::napi_env, data: *mut c_void, _hint: *mut c_void) {
        let ref_ = data as sys::napi_ref;
        let _ = sys::napi_delete_reference(env, ref_);
      }

      // N-API 9+: don't touch the env when we're already outside its lifetime
      if THREADS_CAN_ACCESS_ENV.with(|c| !c.get()) {
        return;
      }

      let status =
        sys::node_api_post_finalizer(env.cast(), Some(do_delete), ref_.cast(), ptr::null_mut());
      trace_drop!(
        "queued for post-finalizer ({:?})  status={:?}",
        ref_,
        status
      );

      if status != sys::Status::napi_ok {
        #[cfg(feature = "trace-buffer-drops")]
        eprintln!(
          "[buffer-drop] post-finalizer failed for {:?} with status {:?}",
          ref_, status
        );
        std::process::abort();
      }
    }

    // N-API ≤ 8 (old trampoline)
    #[cfg(all(
      not(any(feature = "napi9", feature = "napi10")),
      any(
        feature = "napi1",
        feature = "napi2",
        feature = "napi3",
        feature = "napi4",
        feature = "napi5",
        feature = "napi6",
        feature = "napi7",
        feature = "napi8"
      ),
      not(feature = "noop")
    ))]
    {
      if CUSTOM_GC_TSFN_DESTROYED.load(std::sync::atomic::Ordering::SeqCst) {
        return;
      }
      if !THREADS_CAN_ACCESS_ENV.with(|cell| cell.get()) {
        let status = unsafe {
          sys::napi_call_threadsafe_function(
            CUSTOM_GC_TSFN.load(std::sync::atomic::Ordering::SeqCst),
            ref_.cast(),
            1,
          )
        };
        assert!(
          status == sys::Status::napi_ok || status == sys::Status::napi_closing,
          "Call custom GC in Buffer::drop failed {}",
          Status::from(status)
        );
      }
    }
  }
}

/// SAFETY: This is undefined behavior, as the JS side may always modify the underlying buffer,
/// without synchronization. Also see the docs for the `AsMut` impl.
unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

impl Default for Buffer {
  fn default() -> Self {
    Self::from(Vec::default())
  }
}

impl From<Vec<u8>> for Buffer {
  fn from(mut data: Vec<u8>) -> Self {
    let inner_ptr = data.as_mut_ptr();
    #[cfg(all(debug_assertions, not(windows)))]
    register_backing_ptr(inner_ptr);
    let len = data.len();
    let capacity = data.capacity();
    mem::forget(data);
    Buffer {
      // SAFETY: `Vec`'s docs guarantee that its pointer is never null (it's a dangling ptr if not
      // allocated):
      // > The pointer will never be null, so this type is null-pointer-optimized.
      inner: unsafe { NonNull::new_unchecked(inner_ptr) },
      len,
      capacity,
      raw: None,
      owned_by_rust: true,
    }
  }
}

impl From<Buffer> for Vec<u8> {
  fn from(buf: Buffer) -> Self {
    buf.as_ref().to_vec()
  }
}

impl From<&[u8]> for Buffer {
  fn from(inner: &[u8]) -> Self {
    Buffer::from(inner.to_owned())
  }
}

impl From<String> for Buffer {
  fn from(inner: String) -> Self {
    Buffer::from(inner.into_bytes())
  }
}

impl AsRef<[u8]> for Buffer {
  fn as_ref(&self) -> &[u8] {
    // SAFETY: the pointer is guaranteed to be non-null, and guaranteed to be valid if `len` is not 0.
    unsafe { slice::from_raw_parts(self.inner.as_ptr(), self.len) }
  }
}

impl AsMut<[u8]> for Buffer {
  fn as_mut(&mut self) -> &mut [u8] {
    // SAFETY: This is literally undefined behavior. `Buffer::clone` allows you to create shared
    // access to the underlying data, but `as_mut` and `deref_mut` allow unsynchronized mutation of
    // that data (not to speak of the JS side having write access as well, at the same time).
    unsafe { slice::from_raw_parts_mut(self.inner.as_ptr(), self.len) }
  }
}

impl Deref for Buffer {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    self.as_ref()
  }
}

impl DerefMut for Buffer {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.as_mut()
  }
}

impl TypeName for Buffer {
  fn type_name() -> &'static str {
    "Vec<u8>"
  }

  fn value_type() -> ValueType {
    ValueType::Object
  }
}

impl FromNapiValue for Buffer {
  unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
    let mut buf = ptr::null_mut();
    let mut len = 0usize;

    check_status!(
      sys::napi_get_buffer_info(env, napi_val, &mut buf, &mut len),
      "Failed to get Buffer pointer and length"
    )?;

    // Tell Drop that JS owns the memory
    let mut reference = ptr::null_mut();
    check_status!(
      sys::napi_create_reference(env, napi_val, /*refcount*/ 1, &mut reference),
      "Failed to create Buffer reference"
    )?;

    Ok(Self {
      inner: if len == 0 {
        NonNull::dangling()
      } else {
        NonNull::new_unchecked(buf.cast())
      },
      len,
      capacity: len,
      raw: Some((reference, env)),
      owned_by_rust: false,
    })
  }
}

impl ToNapiValue for Buffer {
  unsafe fn to_napi_value(env: sys::napi_env, mut val: Self) -> Result<sys::napi_value> {
    // From Node.js value, not from `Vec<u8>`
    if let Some((ref_, _)) = val.raw {
      let mut buf = ptr::null_mut();
      check_status!(
        unsafe { sys::napi_get_reference_value(env, ref_, &mut buf) },
        "Failed to get Buffer value from reference"
      )?;

      check_status!(
        unsafe { sys::napi_delete_reference(env, ref_) },
        "Failed to delete Buffer reference in Buffer::to_napi_value"
      )?;
      val.raw = Some((ptr::null_mut(), ptr::null_mut()));
      return Ok(buf);
    }
    let len = val.len;
    let mut ret = ptr::null_mut();
    check_status!(
      if len == 0 {
        // Rust uses 0x1 as the data pointer for empty buffers,
        // but NAPI/V8 only allows multiple buffers to have
        // the same data pointer if it's 0x0.
        unsafe { sys::napi_create_buffer(env, len, ptr::null_mut(), &mut ret) }
      } else {
        let value_ptr = val.inner.as_ptr();
        let val_box_ptr = Box::into_raw(Box::new(val));
        // Rust no longer owns the bytes – finaliser will free them
        (*(val_box_ptr)).owned_by_rust = false;
        let mut status = unsafe {
          sys::napi_create_external_buffer(
            env,
            len,
            value_ptr.cast(),
            Some(drop_buffer),
            val_box_ptr.cast(),
            &mut ret,
          )
        };
        if status == napi_sys::Status::napi_no_external_buffers_allowed {
          let value = unsafe { Box::from_raw(val_box_ptr) };
          status = unsafe {
            sys::napi_create_buffer_copy(
              env,
              len,
              value.inner.as_ptr() as *mut c_void,
              ptr::null_mut(),
              &mut ret,
            )
          };
        }
        status
      },
      "Failed to create napi buffer"
    )?;

    Ok(ret)
  }
}

impl ValidateNapiValue for Buffer {
  unsafe fn validate(env: sys::napi_env, napi_val: sys::napi_value) -> Result<sys::napi_value> {
    let mut is_buffer = false;
    check_status!(
      unsafe { sys::napi_is_buffer(env, napi_val, &mut is_buffer) },
      "Failed to validate napi buffer"
    )?;
    if !is_buffer {
      return Err(Error::new(
        Status::InvalidArg,
        "Expected a Buffer value".to_owned(),
      ));
    }
    Ok(ptr::null_mut())
  }
}
