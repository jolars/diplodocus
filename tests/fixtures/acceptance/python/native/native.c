#include <Python.h>
#include <structmember.h>

typedef struct {
  PyObject_HEAD
  long dimension;
} NativeWorkspace;

static int workspace_init(NativeWorkspace *self, PyObject *args, PyObject *kwargs) {
  static char *keywords[] = {"dimension", NULL};
  return PyArg_ParseTupleAndKeywords(args, kwargs, "l", keywords, &self->dimension)
             ? 0
             : -1;
}

static PyMemberDef workspace_members[] = {
    {"dimension", T_LONG, offsetof(NativeWorkspace, dimension), 0, NULL},
    {NULL, 0, 0, 0, NULL},
};

static PyTypeObject workspace_type = {
    PyVarObject_HEAD_INIT(NULL, 0)
    .tp_name = "foo._native.NativeWorkspace",
    .tp_basicsize = sizeof(NativeWorkspace),
    .tp_flags = Py_TPFLAGS_DEFAULT,
    .tp_new = PyType_GenericNew,
    .tp_init = (initproc)workspace_init,
    .tp_members = workspace_members,
};

static PyObject *native_mean(PyObject *self, PyObject *values) {
  PyObject *sequence = PySequence_Fast(values, "values must be a sequence");
  if (sequence == NULL) {
    return NULL;
  }
  Py_ssize_t length = PySequence_Fast_GET_SIZE(sequence);
  if (length == 0) {
    Py_DECREF(sequence);
    PyErr_SetString(PyExc_ValueError, "values must not be empty");
    return NULL;
  }
  double sum = 0;
  for (Py_ssize_t index = 0; index < length; index++) {
    sum += PyFloat_AsDouble(PySequence_Fast_GET_ITEM(sequence, index));
    if (PyErr_Occurred()) {
      Py_DECREF(sequence);
      return NULL;
    }
  }
  Py_DECREF(sequence);
  return PyFloat_FromDouble(sum / length);
}

static PyMethodDef methods[] = {
    {"native_mean", native_mean, METH_O, NULL},
    {NULL, NULL, 0, NULL},
};

static struct PyModuleDef module = {
    PyModuleDef_HEAD_INIT,
    .m_name = "_native",
    .m_size = -1,
    .m_methods = methods,
};

PyMODINIT_FUNC PyInit__native(void) {
  if (PyType_Ready(&workspace_type) < 0) {
    return NULL;
  }
  PyObject *result = PyModule_Create(&module);
  if (result != NULL && PyModule_AddObjectRef(result, "NativeWorkspace",
                                            (PyObject *)&workspace_type) < 0) {
    Py_DECREF(result);
    return NULL;
  }
  return result;
}
