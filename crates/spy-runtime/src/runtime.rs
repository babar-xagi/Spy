use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use pyo3::call::PyCallArgs;
use pyo3::conversion::FromPyObjectOwned;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyModule};
use pyo3::BoundObject;

use crate::RuntimeError;

/// Convenience result alias for SPY Python runtime operations.
pub type SpyResult<T> = Result<T, RuntimeError>;

/// Metadata about the active embedded CPython interpreter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInfo {
    pub version: String,
    pub executable: String,
}

static MODULE_CACHE: OnceLock<Mutex<HashMap<String, Py<PyModule>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, Py<PyModule>>> {
    MODULE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Core CPython lifecycle and GIL controller backed by PyO3.
pub struct SpyRuntime;

impl SpyRuntime {
    /// Ensures the embedded CPython interpreter is initialized and returns runtime info.
    pub fn initialize() -> SpyResult<RuntimeInfo> {
        Python::initialize();
        Self::try_with_gil(|py| {
            let sys = PyModule::import(py, "sys")?;
            let version: String = sys.getattr("version")?.extract()?;
            let executable: String = sys.getattr("executable")?.extract()?;
            Ok(RuntimeInfo {
                version,
                executable,
            })
        })
    }

    /// Returns `true` if CPython is currently initialized in this process.
    pub fn is_initialized() -> bool {
        // SAFETY: `Py_IsInitialized` is safe to call at any point in CPython's lifecycle.
        unsafe { pyo3::ffi::Py_IsInitialized() != 0 }
    }

    /// Attaches to the Python GIL for the duration of `f`.
    pub fn with_gil<F, R>(f: F) -> R
    where
        F: for<'py> FnOnce(Python<'py>) -> R,
    {
        Python::initialize();
        Python::attach(f)
    }

    /// Attaches to the Python GIL and translates any `PyErr` into `RuntimeError`.
    pub fn try_with_gil<F, R>(f: F) -> SpyResult<R>
    where
        F: for<'py> FnOnce(Python<'py>) -> PyResult<R>,
    {
        Self::with_gil(|py| f(py).map_err(|err| RuntimeError::from_py_err(py, err)))
    }

    /// Releases the GIL while running CPU-bound native Rust code.
    pub fn detach<F, R>(py: Python<'_>, f: F) -> R
    where
        F: pyo3::marker::Ungil + FnOnce() -> R,
        R: pyo3::marker::Ungil,
    {
        py.detach(f)
    }
}

/// Owned handle to a Python object that can be safely stored and passed across Rust/Python boundaries.
pub struct SpyObject {
    inner: Py<PyAny>,
}

impl SpyObject {
    /// Wraps an unbound `Py<PyAny>`.
    pub fn from_py_any(inner: Py<PyAny>) -> Self {
        Self { inner }
    }

    /// Converts a Rust value into a `SpyObject`.
    pub fn from_rust<T>(value: T) -> SpyResult<Self>
    where
        T: for<'py> IntoPyObject<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            let bound = value.into_pyobject(py).map_err(Into::into)?;
            Ok(Self {
                inner: bound.into_any().unbind(),
            })
        })
    }

    /// Returns a reference to the underlying `Py<PyAny>`.
    pub fn as_py(&self) -> &Py<PyAny> {
        &self.inner
    }

    /// Extracts this Python object into a Rust value.
    pub fn extract<R>(&self) -> SpyResult<R>
    where
        R: for<'py> FromPyObjectOwned<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            self.inner
                .bind(py)
                .extract::<R>()
                .map_err(|e| -> PyErr { e.into() })
        })
    }

    /// Reads an attribute as another `SpyObject`.
    pub fn attr(&self, name: &str) -> SpyResult<SpyObject> {
        SpyRuntime::try_with_gil(|py| {
            let val = self.inner.bind(py).getattr(name)?;
            Ok(SpyObject::from_py_any(val.unbind()))
        })
    }

    /// Reads and converts an attribute into a Rust type `R`.
    pub fn get_attr<R>(&self, name: &str) -> SpyResult<R>
    where
        R: for<'py> FromPyObjectOwned<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            self.inner
                .bind(py)
                .getattr(name)?
                .extract::<R>()
                .map_err(|e| -> PyErr { e.into() })
        })
    }

    /// Sets an attribute on this Python object.
    pub fn set_attr<V>(&self, name: &str, value: V) -> SpyResult<()>
    where
        V: for<'py> IntoPyObject<'py>,
    {
        SpyRuntime::try_with_gil(|py| self.inner.bind(py).setattr(name, value))
    }

    /// Calls a method on this object with positional arguments and converts the result to `R`.
    pub fn call_method<A, R>(&self, name: &str, args: A) -> SpyResult<R>
    where
        A: for<'py> PyCallArgs<'py>,
        R: for<'py> FromPyObjectOwned<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            self.inner
                .bind(py)
                .call_method1(name, args)?
                .extract::<R>()
                .map_err(|e| -> PyErr { e.into() })
        })
    }

    /// Calls a zero-argument method on this object and converts the result to `R`.
    pub fn call_method0<R>(&self, name: &str) -> SpyResult<R>
    where
        R: for<'py> FromPyObjectOwned<'py>,
    {
        self.call_method(name, ())
    }

    /// Calls a method on this object and returns the raw `SpyObject` result.
    pub fn method<A>(&self, name: &str, args: A) -> SpyResult<SpyObject>
    where
        A: for<'py> PyCallArgs<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            let res = self.inner.bind(py).call_method1(name, args)?;
            Ok(SpyObject::from_py_any(res.unbind()))
        })
    }

    /// Calls a zero-argument method on this object and returns the raw `SpyObject` result.
    pub fn method0(&self, name: &str) -> SpyResult<SpyObject> {
        self.method(name, ())
    }

    /// Calls this object directly (when callable) and extracts `R`.
    pub fn call<A, R>(&self, args: A) -> SpyResult<R>
    where
        A: for<'py> PyCallArgs<'py>,
        R: for<'py> FromPyObjectOwned<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            self.inner
                .bind(py)
                .call1(args)?
                .extract::<R>()
                .map_err(|e| -> PyErr { e.into() })
        })
    }

    /// Calls this object directly (when callable) and returns a `SpyObject`.
    pub fn call_obj<A>(&self, args: A) -> SpyResult<SpyObject>
    where
        A: for<'py> PyCallArgs<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            let res = self.inner.bind(py).call1(args)?;
            Ok(SpyObject::from_py_any(res.unbind()))
        })
    }

    /// Indexes into this object (`self[key]`).
    pub fn get_item<K>(&self, key: K) -> SpyResult<SpyObject>
    where
        K: for<'py> IntoPyObject<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            let res = self.inner.bind(py).get_item(key)?;
            Ok(SpyObject::from_py_any(res.unbind()))
        })
    }

    /// Assigns an item on this object (`self[key] = value`).
    pub fn set_item<K, V>(&self, key: K, value: V) -> SpyResult<()>
    where
        K: for<'py> IntoPyObject<'py>,
        V: for<'py> IntoPyObject<'py>,
    {
        SpyRuntime::try_with_gil(|py| self.inner.bind(py).set_item(key, value))
    }

    /// Returns the Python type name (`type(self).__name__`).
    pub fn type_name(&self) -> SpyResult<String> {
        SpyRuntime::try_with_gil(|py| {
            let name = self.inner.bind(py).get_type().name()?.to_string();
            Ok(name)
        })
    }

    /// Returns `repr(self)`.
    pub fn repr(&self) -> SpyResult<String> {
        SpyRuntime::try_with_gil(|py| {
            let repr = self.inner.bind(py).repr()?.to_string();
            Ok(repr)
        })
    }

    /// Returns `str(self)`.
    pub fn str_value(&self) -> SpyResult<String> {
        SpyRuntime::try_with_gil(|py| {
            let s = self.inner.bind(py).str()?.to_string();
            Ok(s)
        })
    }
}

impl Clone for SpyObject {
    fn clone(&self) -> Self {
        SpyRuntime::with_gil(|py| Self {
            inner: self.inner.clone_ref(py),
        })
    }
}

impl fmt::Debug for SpyObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.repr() {
            Ok(r) => write!(f, "SpyObject({r})"),
            Err(e) => write!(f, "SpyObject(<repr error: {e}>)"),
        }
    }
}

impl fmt::Display for SpyObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.str_value() {
            Ok(s) => f.write_str(&s),
            Err(e) => write!(f, "<python str error: {e}>"),
        }
    }
}

impl<'py> IntoPyObject<'py> for SpyObject {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(self.inner.into_bound(py))
    }
}

impl<'a, 'py> IntoPyObject<'py> for &'a SpyObject {
    type Target = PyAny;
    type Output = Borrowed<'a, 'py, PyAny>;
    type Error = Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(self.inner.bind_borrowed(py))
    }
}

/// Handle to an imported Python module.
pub struct SpyModule {
    name: String,
    inner: Py<PyModule>,
}

impl SpyModule {
    /// Returns the dotted module name (e.g. `"pandas"` or `"os.path"`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the underlying `Py<PyModule>`.
    pub fn as_py(&self) -> &Py<PyModule> {
        &self.inner
    }

    /// Converts this module handle into a generic `SpyObject`.
    pub fn as_object(&self) -> SpyObject {
        SpyRuntime::with_gil(|py| SpyObject::from_py_any(self.inner.clone_ref(py).into_any()))
    }

    /// Reads an attribute (function, class, constant, submodule) as a `SpyObject`.
    pub fn get(&self, attr_name: &str) -> SpyResult<SpyObject> {
        SpyRuntime::try_with_gil(|py| {
            let val = self.inner.bind(py).getattr(attr_name)?;
            Ok(SpyObject::from_py_any(val.unbind()))
        })
    }

    /// Reads and extracts a module attribute directly into a Rust type `R`.
    pub fn attr<R>(&self, attr_name: &str) -> SpyResult<R>
    where
        R: for<'py> FromPyObjectOwned<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            self.inner
                .bind(py)
                .getattr(attr_name)?
                .extract::<R>()
                .map_err(|e| -> PyErr { e.into() })
        })
    }

    /// Calls a module function with positional arguments and extracts the return value as `R`.
    pub fn call<A, R>(&self, fn_name: &str, args: A) -> SpyResult<R>
    where
        A: for<'py> PyCallArgs<'py>,
        R: for<'py> FromPyObjectOwned<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            self.inner
                .bind(py)
                .getattr(fn_name)?
                .call1(args)?
                .extract::<R>()
                .map_err(|e| -> PyErr { e.into() })
        })
    }

    /// Calls a zero-argument module function and extracts the return value as `R`.
    pub fn call0<R>(&self, fn_name: &str) -> SpyResult<R>
    where
        R: for<'py> FromPyObjectOwned<'py>,
    {
        self.call(fn_name, ())
    }

    /// Calls a module function with positional arguments and returns a `SpyObject`.
    pub fn call_obj<A>(&self, fn_name: &str, args: A) -> SpyResult<SpyObject>
    where
        A: for<'py> PyCallArgs<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            let res = self.inner.bind(py).getattr(fn_name)?.call1(args)?;
            Ok(SpyObject::from_py_any(res.unbind()))
        })
    }

    /// Calls a module function with positional and keyword arguments, returning a `SpyObject`.
    pub fn call_kw<A, V>(&self, fn_name: &str, args: A, kwargs: &[(&str, V)]) -> SpyResult<SpyObject>
    where
        A: for<'py> PyCallArgs<'py>,
        V: Clone + for<'py> IntoPyObject<'py>,
    {
        SpyRuntime::try_with_gil(|py| {
            let dict = PyDict::new(py);
            for (k, v) in kwargs {
                dict.set_item(*k, v.clone())?;
            }
            let func = self.inner.bind(py).getattr(fn_name)?;
            let res = call_with_optional_kwargs(&func, args, &dict)?;
            Ok(SpyObject::from_py_any(res.unbind()))
        })
    }

    /// Instantiates a Python class defined in this module.
    pub fn instantiate<A>(&self, class_name: &str, args: A) -> SpyResult<SpyObject>
    where
        A: for<'py> PyCallArgs<'py>,
    {
        self.call_obj(class_name, args)
    }

    /// Instantiates a Python class defined in this module with no arguments.
    pub fn instantiate0(&self, class_name: &str) -> SpyResult<SpyObject> {
        self.call_obj(class_name, ())
    }
}

fn call_with_optional_kwargs<'py, A: PyCallArgs<'py>>(
    callable: &Bound<'py, PyAny>,
    args: A,
    kwargs: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyAny>> {
    if kwargs.is_empty() {
        callable.call1(args)
    } else {
        callable.call(args, Some(kwargs))
    }
}

impl Clone for SpyModule {
    fn clone(&self) -> Self {
        SpyRuntime::with_gil(|py| Self {
            name: self.name.clone(),
            inner: self.inner.clone_ref(py),
        })
    }
}

impl fmt::Debug for SpyModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SpyModule({:?})", self.name)
    }
}

impl<'py> IntoPyObject<'py> for SpyModule {
    type Target = PyModule;
    type Output = Bound<'py, PyModule>;
    type Error = Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(self.inner.into_bound(py))
    }
}

impl<'a, 'py> IntoPyObject<'py> for &'a SpyModule {
    type Target = PyModule;
    type Output = Borrowed<'a, 'py, PyModule>;
    type Error = Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(self.inner.bind_borrowed(py))
    }
}

/// Const-constructible module binding emitted by `spy-codegen` for `use python::...;`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LazyPyModule {
    module_name: &'static str,
}

impl LazyPyModule {
    /// Creates a compile-time module reference for a dotted Python module path.
    pub const fn new(module_name: &'static str) -> Self {
        Self { module_name }
    }

    /// Returns the dotted module name.
    pub const fn name(&self) -> &'static str {
        self.module_name
    }

    /// Imports (or fetches from the SPY module cache) the underlying `SpyModule`.
    pub fn module(&self) -> SpyResult<SpyModule> {
        import_module(self.module_name)
    }

    pub fn get(&self, attr_name: &str) -> SpyResult<SpyObject> {
        self.module()?.get(attr_name)
    }

    pub fn attr<R>(&self, attr_name: &str) -> SpyResult<R>
    where
        R: for<'py> FromPyObjectOwned<'py>,
    {
        self.module()?.attr(attr_name)
    }

    pub fn call<A, R>(&self, fn_name: &str, args: A) -> SpyResult<R>
    where
        A: for<'py> PyCallArgs<'py>,
        R: for<'py> FromPyObjectOwned<'py>,
    {
        self.module()?.call(fn_name, args)
    }

    pub fn call0<R>(&self, fn_name: &str) -> SpyResult<R>
    where
        R: for<'py> FromPyObjectOwned<'py>,
    {
        self.module()?.call0(fn_name)
    }

    pub fn call_obj<A>(&self, fn_name: &str, args: A) -> SpyResult<SpyObject>
    where
        A: for<'py> PyCallArgs<'py>,
    {
        self.module()?.call_obj(fn_name, args)
    }

    pub fn call_kw<A, V>(&self, fn_name: &str, args: A, kwargs: &[(&str, V)]) -> SpyResult<SpyObject>
    where
        A: for<'py> PyCallArgs<'py>,
        V: Clone + for<'py> IntoPyObject<'py>,
    {
        self.module()?.call_kw(fn_name, args, kwargs)
    }

    pub fn instantiate<A>(&self, class_name: &str, args: A) -> SpyResult<SpyObject>
    where
        A: for<'py> PyCallArgs<'py>,
    {
        self.module()?.instantiate(class_name, args)
    }

    pub fn instantiate0(&self, class_name: &str) -> SpyResult<SpyObject> {
        self.module()?.instantiate0(class_name)
    }
}

/// Imports a Python module, returning a cached handle when already imported.
pub fn import_module(module_name: &str) -> SpyResult<SpyModule> {
    Python::initialize();

    {
        let guard = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = guard.get(module_name) {
            let cloned = Python::attach(|py| cached.clone_ref(py));
            return Ok(SpyModule {
                name: module_name.to_owned(),
                inner: cloned,
            });
        }
    }

    let imported = import_module_uncached(module_name)?;
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    let stored = Python::attach(|py| {
        guard
            .entry(module_name.to_owned())
            .or_insert_with(|| imported.inner.clone_ref(py))
            .clone_ref(py)
    });

    Ok(SpyModule {
        name: module_name.to_owned(),
        inner: stored,
    })
}

/// Imports a Python module directly without consulting or updating the SPY module cache.
pub fn import_module_uncached(module_name: &str) -> SpyResult<SpyModule> {
    Python::initialize();
    Python::attach(|py| match PyModule::import(py, module_name) {
        Ok(module) => Ok(SpyModule {
            name: module_name.to_owned(),
            inner: module.unbind(),
        }),
        Err(err) => Err(build_import_error(py, module_name, err)),
    })
}

/// Eagerly imports and caches a slice of module names.
pub fn preload_modules(modules: &[&str]) -> SpyResult<()> {
    for module_name in modules {
        import_module(module_name)?;
    }
    Ok(())
}

/// Eagerly imports and caches modules, printing a formatted diagnostic and exiting on failure.
pub fn preload_or_exit(modules: &[&str]) {
    if let Err(err) = preload_modules(modules) {
        eprintln!("spy runtime error: {err}");
        std::process::exit(1);
    }
}

/// Evaluates inline Python source code as a named module (useful for embedded helpers and tests).
pub fn eval_module_code(code: &std::ffi::CStr, file_name: &std::ffi::CStr, module_name: &str) -> SpyResult<SpyModule> {
    SpyRuntime::try_with_gil(|py| {
        let mod_cstr = std::ffi::CString::new(module_name).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("invalid module name: {e}"))
        })?;
        let module = PyModule::from_code(py, code, file_name, &mod_cstr)?;
        Ok(SpyModule {
            name: module_name.to_owned(),
            inner: module.unbind(),
        })
    })
}

/// Returns the sorted list of module names currently stored in SPY's module cache.
pub fn cached_module_names() -> Vec<String> {
    let guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    let mut names: Vec<String> = guard.keys().cloned().collect();
    names.sort();
    names
}

/// Clears SPY's cached `SpyModule` handles (primarily for tests).
pub fn clear_module_cache() {
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    guard.clear();
}

fn build_import_error(py: Python<'_>, module_name: &str, err: PyErr) -> RuntimeError {
    let reason = err.value(py).str().map(|s| s.to_string()).unwrap_or_else(|_| err.to_string());
    let (python_executable, python_version) = match PyModule::import(py, "sys") {
        Ok(sys) => {
            let exe = sys
                .getattr("executable")
                .and_then(|v| v.extract::<String>())
                .unwrap_or_else(|_| "<unknown>".to_owned());
            let ver = sys
                .getattr("version")
                .and_then(|v| v.extract::<String>())
                .map(|v| v.split_whitespace().next().unwrap_or("").to_owned())
                .unwrap_or_else(|_| "<unknown>".to_owned());
            (exe, ver)
        }
        Err(_) => ("<unknown>".to_owned(), "<unknown>".to_owned()),
    };

    let root_pkg = module_name.split('.').next().unwrap_or(module_name);
    let hint = format!(
        "ensure `{root_pkg}` is installed in `{python_executable}` (e.g. `pip install {root_pkg}` or `uv pip install {root_pkg}`), or set `PYO3_PYTHON` / activate the target virtual environment"
    );

    RuntimeError::ImportFailed {
        module: module_name.to_owned(),
        python_executable,
        python_version,
        reason,
        hint,
    }
}

pub(crate) fn extract_py_err_details(py: Python<'_>, err: PyErr) -> (String, String, Option<String>) {
    let exc_type = err
        .get_type(py)
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| "PythonException".to_owned());
    let message = err
        .value(py)
        .str()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| err.to_string());

    let traceback = format_traceback(py, &err);
    (exc_type, message, traceback)
}

fn format_traceback(py: Python<'_>, err: &PyErr) -> Option<String> {
    let tb_mod = PyModule::import(py, "traceback").ok()?;
    let tb_obj = err.traceback(py)?;
    let lines: Vec<String> = tb_mod
        .getattr("format_tb")
        .ok()?
        .call1((tb_obj,))
        .ok()?
        .extract()
        .ok()?;
    let joined = lines.concat();
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}
