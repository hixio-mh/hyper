use bytes::Bytes;
use libc::{c_int, size_t};
use std::ffi::c_void;

use super::body::{hyper_body, hyper_buf};
use super::error::hyper_code;
use super::task::{hyper_task_return_type, AsTaskType};
use super::HYPER_ITER_CONTINUE;
use crate::ext::HeaderCaseMap;
use crate::header::{HeaderName, HeaderValue};
use crate::{Body, HeaderMap, Method, Request, Response, Uri};

/// An HTTP request.
pub struct hyper_request(pub(super) Request<Body>);

/// An HTTP response.
pub struct hyper_response(pub(super) Response<Body>);

/// An HTTP header map.
///
/// These can be part of a request or response.
pub struct hyper_headers {
    pub(super) headers: HeaderMap,
    orig_casing: HeaderCaseMap,
}

#[derive(Debug)]
pub(crate) struct ReasonPhrase(pub(crate) Bytes);

pub(crate) struct RawHeaders(pub(crate) hyper_buf);

// ===== impl hyper_request =====

ffi_fn! {
    /// Construct a new HTTP request.
    fn hyper_request_new() -> *mut hyper_request {
        Box::into_raw(Box::new(hyper_request(Request::new(Body::empty()))))
    } ?= std::ptr::null_mut()
}

ffi_fn! {
    /// Free an HTTP request if not going to send it on a client.
    fn hyper_request_free(req: *mut hyper_request) {
        drop(unsafe { Box::from_raw(req) });
    }
}

ffi_fn! {
    /// Set the HTTP Method of the request.
    fn hyper_request_set_method(req: *mut hyper_request, method: *const u8, method_len: size_t) -> hyper_code {
        let bytes = unsafe {
            std::slice::from_raw_parts(method, method_len as usize)
        };
        match Method::from_bytes(bytes) {
            Ok(m) => {
                *unsafe { &mut *req }.0.method_mut() = m;
                hyper_code::HYPERE_OK
            },
            Err(_) => {
                hyper_code::HYPERE_INVALID_ARG
            }
        }
    }
}

ffi_fn! {
    /// Set the URI of the request.
    fn hyper_request_set_uri(req: *mut hyper_request, uri: *const u8, uri_len: size_t) -> hyper_code {
        let bytes = unsafe {
            std::slice::from_raw_parts(uri, uri_len as usize)
        };
        match Uri::from_maybe_shared(bytes) {
            Ok(u) => {
                *unsafe { &mut *req }.0.uri_mut() = u;
                hyper_code::HYPERE_OK
            },
            Err(_) => {
                hyper_code::HYPERE_INVALID_ARG
            }
        }
    }
}

ffi_fn! {
    /// Set the preferred HTTP version of the request.
    ///
    /// The version value should be one of the `HYPER_HTTP_VERSION_` constants.
    ///
    /// Note that this won't change the major HTTP version of the connection,
    /// since that is determined at the handshake step.
    fn hyper_request_set_version(req: *mut hyper_request, version: c_int) -> hyper_code {
        use http::Version;

        *unsafe { &mut *req }.0.version_mut() = match version {
            super::HYPER_HTTP_VERSION_NONE => Version::HTTP_11,
            super::HYPER_HTTP_VERSION_1_0 => Version::HTTP_10,
            super::HYPER_HTTP_VERSION_1_1 => Version::HTTP_11,
            super::HYPER_HTTP_VERSION_2 => Version::HTTP_2,
            _ => {
                // We don't know this version
                return hyper_code::HYPERE_INVALID_ARG;
            }
        };
        hyper_code::HYPERE_OK
    }
}

ffi_fn! {
    /// Gets a reference to the HTTP headers of this request
    ///
    /// This is not an owned reference, so it should not be accessed after the
    /// `hyper_request` has been consumed.
    fn hyper_request_headers(req: *mut hyper_request) -> *mut hyper_headers {
        hyper_headers::get_or_default(unsafe { &mut *req }.0.extensions_mut())
    } ?= std::ptr::null_mut()
}

ffi_fn! {
    /// Set the body of the request.
    ///
    /// The default is an empty body.
    ///
    /// This takes ownership of the `hyper_body *`, you must not use it or
    /// free it after setting it on the request.
    fn hyper_request_set_body(req: *mut hyper_request, body: *mut hyper_body) -> hyper_code {
        let body = unsafe { Box::from_raw(body) };
        *unsafe { &mut *req }.0.body_mut() = body.0;
        hyper_code::HYPERE_OK
    }
}

impl hyper_request {
    pub(super) fn finalize_request(&mut self) {
        if let Some(headers) = self.0.extensions_mut().remove::<hyper_headers>() {
            *self.0.headers_mut() = headers.headers;
            self.0.extensions_mut().insert(headers.orig_casing);
        }
    }
}

// ===== impl hyper_response =====

ffi_fn! {
    /// Free an HTTP response after using it.
    fn hyper_response_free(resp: *mut hyper_response) {
        drop(unsafe { Box::from_raw(resp) });
    }
}

ffi_fn! {
    /// Get the HTTP-Status code of this response.
    ///
    /// It will always be within the range of 100-599.
    fn hyper_response_status(resp: *const hyper_response) -> u16 {
        unsafe { &*resp }.0.status().as_u16()
    }
}

ffi_fn! {
    /// Get a pointer to the reason-phrase of this response.
    ///
    /// This buffer is not null-terminated.
    ///
    /// This buffer is owned by the response, and should not be used after
    /// the response has been freed.
    ///
    /// Use `hyper_response_reason_phrase_len()` to get the length of this
    /// buffer.
    fn hyper_response_reason_phrase(resp: *const hyper_response) -> *const u8 {
        unsafe { &*resp }.reason_phrase().as_ptr()
    } ?= std::ptr::null()
}

ffi_fn! {
    /// Get the length of the reason-phrase of this response.
    ///
    /// Use `hyper_response_reason_phrase()` to get the buffer pointer.
    fn hyper_response_reason_phrase_len(resp: *const hyper_response) -> size_t {
        unsafe { &*resp }.reason_phrase().len()
    }
}

ffi_fn! {
    /// Get a reference to the full raw headers of this response.
    ///
    /// You must have enabled `hyper_clientconn_options_headers_raw()`, or this
    /// will return NULL.
    ///
    /// The returned `hyper_buf *` is just a reference, owned by the response.
    /// You need to make a copy if you wish to use it after freeing the
    /// response.
    ///
    /// The buffer is not null-terminated, see the `hyper_buf` functions for
    /// getting the bytes and length.
    fn hyper_response_headers_raw(resp: *const hyper_response) -> *const hyper_buf {
        match unsafe { &*resp }.0.extensions().get::<RawHeaders>() {
            Some(raw) => &raw.0,
            None => std::ptr::null(),
        }
    } ?= std::ptr::null()
}

ffi_fn! {
    /// Get the HTTP version used by this response.
    ///
    /// The returned value could be:
    ///
    /// - `HYPER_HTTP_VERSION_1_0`
    /// - `HYPER_HTTP_VERSION_1_1`
    /// - `HYPER_HTTP_VERSION_2`
    /// - `HYPER_HTTP_VERSION_NONE` if newer (or older).
    fn hyper_response_version(resp: *const hyper_response) -> c_int {
        use http::Version;

        match unsafe { &*resp }.0.version() {
            Version::HTTP_10 => super::HYPER_HTTP_VERSION_1_0,
            Version::HTTP_11 => super::HYPER_HTTP_VERSION_1_1,
            Version::HTTP_2 => super::HYPER_HTTP_VERSION_2,
            _ => super::HYPER_HTTP_VERSION_NONE,
        }
    }
}

ffi_fn! {
    /// Gets a reference to the HTTP headers of this response.
    ///
    /// This is not an owned reference, so it should not be accessed after the
    /// `hyper_response` has been freed.
    fn hyper_response_headers(resp: *mut hyper_response) -> *mut hyper_headers {
        hyper_headers::get_or_default(unsafe { &mut *resp }.0.extensions_mut())
    } ?= std::ptr::null_mut()
}

ffi_fn! {
    /// Take ownership of the body of this response.
    ///
    /// It is safe to free the response even after taking ownership of its body.
    fn hyper_response_body(resp: *mut hyper_response) -> *mut hyper_body {
        let body = std::mem::take(unsafe { &mut *resp }.0.body_mut());
        Box::into_raw(Box::new(hyper_body(body)))
    } ?= std::ptr::null_mut()
}

impl hyper_response {
    pub(super) fn wrap(mut resp: Response<Body>) -> hyper_response {
        let headers = std::mem::take(resp.headers_mut());
        let orig_casing = resp
            .extensions_mut()
            .remove::<HeaderCaseMap>()
            .unwrap_or_else(HeaderCaseMap::default);
        resp.extensions_mut().insert(hyper_headers {
            headers,
            orig_casing,
        });

        hyper_response(resp)
    }

    fn reason_phrase(&self) -> &[u8] {
        if let Some(reason) = self.0.extensions().get::<ReasonPhrase>() {
            return &reason.0;
        }

        if let Some(reason) = self.0.status().canonical_reason() {
            return reason.as_bytes();
        }

        &[]
    }
}

unsafe impl AsTaskType for hyper_response {
    fn as_task_type(&self) -> hyper_task_return_type {
        hyper_task_return_type::HYPER_TASK_RESPONSE
    }
}

// ===== impl Headers =====

type hyper_headers_foreach_callback =
    extern "C" fn(*mut c_void, *const u8, size_t, *const u8, size_t) -> c_int;

impl hyper_headers {
    pub(super) fn get_or_default(ext: &mut http::Extensions) -> &mut hyper_headers {
        if let None = ext.get_mut::<hyper_headers>() {
            ext.insert(hyper_headers::default());
        }

        ext.get_mut::<hyper_headers>().unwrap()
    }
}

ffi_fn! {
    /// Iterates the headers passing each name and value pair to the callback.
    ///
    /// The `userdata` pointer is also passed to the callback.
    ///
    /// The callback should return `HYPER_ITER_CONTINUE` to keep iterating, or
    /// `HYPER_ITER_BREAK` to stop.
    fn hyper_headers_foreach(headers: *const hyper_headers, func: hyper_headers_foreach_callback, userdata: *mut c_void) {
        let headers = unsafe { &*headers };
        // For each header name/value pair, there may be a value in the casemap
        // that corresponds to the HeaderValue. So, we iterator all the keys,
        // and for each one, try to pair the originally cased name with the value.
        //
        // TODO: consider adding http::HeaderMap::entries() iterator
        for name in headers.headers.keys() {
            let mut names = headers.orig_casing.get_all(name);

            for value in headers.headers.get_all(name) {
                let (name_ptr, name_len) = if let Some(orig_name) = names.next() {
                    (orig_name.as_ref().as_ptr(), orig_name.as_ref().len())
                } else {
                    (
                        name.as_str().as_bytes().as_ptr(),
                        name.as_str().as_bytes().len(),
                    )
                };

                let val_ptr = value.as_bytes().as_ptr();
                let val_len = value.as_bytes().len();

                if HYPER_ITER_CONTINUE != func(userdata, name_ptr, name_len, val_ptr, val_len) {
                    return;
                }
            }
        }
    }
}

ffi_fn! {
    /// Sets the header with the provided name to the provided value.
    ///
    /// This overwrites any previous value set for the header.
    fn hyper_headers_set(headers: *mut hyper_headers, name: *const u8, name_len: size_t, value: *const u8, value_len: size_t) -> hyper_code {
        let headers = unsafe { &mut *headers };
        match unsafe { raw_name_value(name, name_len, value, value_len) } {
            Ok((name, value, orig_name)) => {
                headers.headers.insert(&name, value);
                headers.orig_casing.insert(name, orig_name);
                hyper_code::HYPERE_OK
            }
            Err(code) => code,
        }
    }
}

ffi_fn! {
    /// Adds the provided value to the list of the provided name.
    ///
    /// If there were already existing values for the name, this will append the
    /// new value to the internal list.
    fn hyper_headers_add(headers: *mut hyper_headers, name: *const u8, name_len: size_t, value: *const u8, value_len: size_t) -> hyper_code {
        let headers = unsafe { &mut *headers };

        match unsafe { raw_name_value(name, name_len, value, value_len) } {
            Ok((name, value, orig_name)) => {
                headers.headers.append(&name, value);
                headers.orig_casing.append(name, orig_name);
                hyper_code::HYPERE_OK
            }
            Err(code) => code,
        }
    }
}

impl Default for hyper_headers {
    fn default() -> Self {
        Self {
            headers: Default::default(),
            orig_casing: HeaderCaseMap::default(),
        }
    }
}

unsafe fn raw_name_value(
    name: *const u8,
    name_len: size_t,
    value: *const u8,
    value_len: size_t,
) -> Result<(HeaderName, HeaderValue, Bytes), hyper_code> {
    let name = std::slice::from_raw_parts(name, name_len);
    let orig_name = Bytes::copy_from_slice(name);
    let name = match HeaderName::from_bytes(name) {
        Ok(name) => name,
        Err(_) => return Err(hyper_code::HYPERE_INVALID_ARG),
    };
    let value = std::slice::from_raw_parts(value, value_len);
    let value = match HeaderValue::from_bytes(value) {
        Ok(val) => val,
        Err(_) => return Err(hyper_code::HYPERE_INVALID_ARG),
    };

    Ok((name, value, orig_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headers_foreach_cases_preserved() {
        let mut headers = hyper_headers::default();

        let name1 = b"Set-CookiE";
        let value1 = b"a=b";
        hyper_headers_add(
            &mut headers,
            name1.as_ptr(),
            name1.len(),
            value1.as_ptr(),
            value1.len(),
        );

        let name2 = b"SET-COOKIE";
        let value2 = b"c=d";
        hyper_headers_add(
            &mut headers,
            name2.as_ptr(),
            name2.len(),
            value2.as_ptr(),
            value2.len(),
        );

        let mut vec = Vec::<u8>::new();
        hyper_headers_foreach(&headers, concat, &mut vec as *mut _ as *mut c_void);

        assert_eq!(vec, b"Set-CookiE: a=b\r\nSET-COOKIE: c=d\r\n");

        extern "C" fn concat(
            vec: *mut c_void,
            name: *const u8,
            name_len: usize,
            value: *const u8,
            value_len: usize,
        ) -> c_int {
            unsafe {
                let vec = &mut *(vec as *mut Vec<u8>);
                let name = std::slice::from_raw_parts(name, name_len);
                let value = std::slice::from_raw_parts(value, value_len);
                vec.extend(name);
                vec.extend(b": ");
                vec.extend(value);
                vec.extend(b"\r\n");
            }
            HYPER_ITER_CONTINUE
        }
    }
}
