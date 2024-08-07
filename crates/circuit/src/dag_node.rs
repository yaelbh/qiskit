// This code is part of Qiskit.
//
// (C) Copyright IBM 2024
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.
//
// Any modifications or derivative works of this code must retain this
// copyright notice, and modified files need to carry a notice indicating
// that they have been altered from the originals.

use crate::circuit_instruction::{
    convert_py_to_operation_type, operation_type_to_py, CircuitInstruction,
    ExtraInstructionAttributes,
};
use crate::imports::QUANTUM_CIRCUIT;
use crate::operations::Operation;
use numpy::IntoPyArray;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PySequence, PyString, PyTuple};
use pyo3::{intern, IntoPy, PyObject, PyResult, ToPyObject};
use smallvec::smallvec;

/// Parent class for DAGOpNode, DAGInNode, and DAGOutNode.
#[pyclass(module = "qiskit._accelerate.circuit", subclass)]
#[derive(Clone, Debug)]
pub struct DAGNode {
    #[pyo3(get, set)]
    pub _node_id: isize,
}

#[pymethods]
impl DAGNode {
    #[new]
    #[pyo3(signature=(nid=-1))]
    fn new(nid: isize) -> Self {
        DAGNode { _node_id: nid }
    }

    fn __getstate__(&self) -> isize {
        self._node_id
    }

    fn __setstate__(&mut self, nid: isize) {
        self._node_id = nid;
    }

    fn __lt__(&self, other: &DAGNode) -> bool {
        self._node_id < other._node_id
    }

    fn __gt__(&self, other: &DAGNode) -> bool {
        self._node_id > other._node_id
    }

    fn __str__(_self: &Bound<DAGNode>) -> String {
        format!("{}", _self.as_ptr() as usize)
    }

    fn __hash__(&self, py: Python) -> PyResult<isize> {
        self._node_id.into_py(py).bind(py).hash()
    }
}

/// Object to represent an Instruction at a node in the DAGCircuit.
#[pyclass(module = "qiskit._accelerate.circuit", extends=DAGNode)]
pub struct DAGOpNode {
    pub instruction: CircuitInstruction,
    #[pyo3(get)]
    pub sort_key: PyObject,
}

#[pymethods]
impl DAGOpNode {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (op, qargs=None, cargs=None, params=smallvec![], label=None, duration=None, unit=None, condition=None, dag=None))]
    fn new(
        py: Python,
        op: crate::circuit_instruction::OperationInput,
        qargs: Option<&Bound<PySequence>>,
        cargs: Option<&Bound<PySequence>>,
        params: smallvec::SmallVec<[crate::operations::Param; 3]>,
        label: Option<String>,
        duration: Option<PyObject>,
        unit: Option<String>,
        condition: Option<PyObject>,
        dag: Option<&Bound<PyAny>>,
    ) -> PyResult<(Self, DAGNode)> {
        let qargs =
            qargs.map_or_else(|| Ok(PyTuple::empty_bound(py)), PySequenceMethods::to_tuple)?;
        let cargs =
            cargs.map_or_else(|| Ok(PyTuple::empty_bound(py)), PySequenceMethods::to_tuple)?;

        let sort_key = match dag {
            Some(dag) => {
                let cache = dag
                    .getattr(intern!(py, "_key_cache"))?
                    .downcast_into_exact::<PyDict>()?;
                let cache_key = PyTuple::new_bound(py, [&qargs, &cargs]);
                match cache.get_item(&cache_key)? {
                    Some(key) => key,
                    None => {
                        let indices: PyResult<Vec<_>> = qargs
                            .iter()
                            .chain(cargs.iter())
                            .map(|bit| {
                                dag.call_method1(intern!(py, "find_bit"), (bit,))?
                                    .getattr(intern!(py, "index"))
                            })
                            .collect();
                        let index_strs: Vec<_> =
                            indices?.into_iter().map(|i| format!("{:04}", i)).collect();
                        let key = PyString::new_bound(py, index_strs.join(",").as_str());
                        cache.set_item(&cache_key, &key)?;
                        key.into_any()
                    }
                }
            }
            None => qargs.str()?.into_any(),
        };

        let mut instruction = CircuitInstruction::py_new(
            py, op, None, None, params, label, duration, unit, condition,
        )?;
        instruction.qubits = qargs.into();
        instruction.clbits = cargs.into();

        Ok((
            DAGOpNode {
                instruction,
                sort_key: sort_key.unbind(),
            },
            DAGNode { _node_id: -1 },
        ))
    }

    #[staticmethod]
    fn from_instruction(
        py: Python,
        instruction: CircuitInstruction,
        dag: Option<&Bound<PyAny>>,
    ) -> PyResult<PyObject> {
        let qargs = instruction.qubits.clone_ref(py).into_bound(py);
        let cargs = instruction.clbits.clone_ref(py).into_bound(py);

        let sort_key = match dag {
            Some(dag) => {
                let cache = dag
                    .getattr(intern!(py, "_key_cache"))?
                    .downcast_into_exact::<PyDict>()?;
                let cache_key = PyTuple::new_bound(py, [&qargs, &cargs]);
                match cache.get_item(&cache_key)? {
                    Some(key) => key,
                    None => {
                        let indices: PyResult<Vec<_>> = qargs
                            .iter()
                            .chain(cargs.iter())
                            .map(|bit| {
                                dag.call_method1(intern!(py, "find_bit"), (bit,))?
                                    .getattr(intern!(py, "index"))
                            })
                            .collect();
                        let index_strs: Vec<_> =
                            indices?.into_iter().map(|i| format!("{:04}", i)).collect();
                        let key = PyString::new_bound(py, index_strs.join(",").as_str());
                        cache.set_item(&cache_key, &key)?;
                        key.into_any()
                    }
                }
            }
            None => qargs.str()?.into_any(),
        };
        let base = PyClassInitializer::from(DAGNode { _node_id: -1 });
        let sub = base.add_subclass(DAGOpNode {
            instruction,
            sort_key: sort_key.unbind(),
        });
        Ok(Py::new(py, sub)?.to_object(py))
    }

    fn __reduce__(slf: PyRef<Self>, py: Python) -> PyResult<PyObject> {
        let state = (slf.as_ref()._node_id, &slf.sort_key);
        Ok((
            py.get_type_bound::<Self>(),
            (
                operation_type_to_py(py, &slf.instruction)?,
                &slf.instruction.qubits,
                &slf.instruction.clbits,
            ),
            state,
        )
            .into_py(py))
    }

    fn __setstate__(mut slf: PyRefMut<Self>, state: &Bound<PyAny>) -> PyResult<()> {
        let (nid, sort_key): (isize, PyObject) = state.extract()?;
        slf.as_mut()._node_id = nid;
        slf.sort_key = sort_key;
        Ok(())
    }

    #[getter]
    fn get_op(&self, py: Python) -> PyResult<PyObject> {
        operation_type_to_py(py, &self.instruction)
    }

    #[setter]
    fn set_op(&mut self, py: Python, op: PyObject) -> PyResult<()> {
        let res = convert_py_to_operation_type(py, op)?;
        self.instruction.operation = res.operation;
        self.instruction.params = res.params;
        let extra_attrs = if res.label.is_some()
            || res.duration.is_some()
            || res.unit.is_some()
            || res.condition.is_some()
        {
            Some(Box::new(ExtraInstructionAttributes {
                label: res.label,
                duration: res.duration,
                unit: res.unit,
                condition: res.condition,
            }))
        } else {
            None
        };
        self.instruction.extra_attrs = extra_attrs;
        Ok(())
    }

    #[getter]
    fn num_qubits(&self) -> u32 {
        self.instruction.operation.num_qubits()
    }

    #[getter]
    fn num_clbits(&self) -> u32 {
        self.instruction.operation.num_clbits()
    }

    #[getter]
    fn get_qargs(&self, py: Python) -> Py<PyTuple> {
        self.instruction.qubits.clone_ref(py)
    }

    #[setter]
    fn set_qargs(&mut self, qargs: Py<PyTuple>) {
        self.instruction.qubits = qargs;
    }

    #[getter]
    fn get_cargs(&self, py: Python) -> Py<PyTuple> {
        self.instruction.clbits.clone_ref(py)
    }

    #[setter]
    fn set_cargs(&mut self, cargs: Py<PyTuple>) {
        self.instruction.clbits = cargs;
    }

    /// Returns the Instruction name corresponding to the op for this node
    #[getter]
    fn get_name(&self) -> &str {
        self.instruction.operation.name()
    }

    #[getter]
    fn get_params(&self, py: Python) -> PyObject {
        self.instruction.params.to_object(py)
    }

    pub fn is_parameterized(&self) -> bool {
        self.instruction.is_parameterized()
    }

    #[getter]
    fn matrix(&self, py: Python) -> Option<PyObject> {
        let matrix = self.instruction.operation.matrix(&self.instruction.params);
        matrix.map(|mat| mat.into_pyarray_bound(py).into())
    }

    #[getter]
    fn label(&self) -> Option<&str> {
        self.instruction
            .extra_attrs
            .as_ref()
            .and_then(|attrs| attrs.label.as_deref())
    }

    #[getter]
    fn condition(&self, py: Python) -> Option<PyObject> {
        self.instruction
            .extra_attrs
            .as_ref()
            .and_then(|attrs| attrs.condition.as_ref().map(|x| x.clone_ref(py)))
    }

    #[getter]
    fn duration(&self, py: Python) -> Option<PyObject> {
        self.instruction
            .extra_attrs
            .as_ref()
            .and_then(|attrs| attrs.duration.as_ref().map(|x| x.clone_ref(py)))
    }

    #[getter]
    fn unit(&self) -> Option<&str> {
        self.instruction
            .extra_attrs
            .as_ref()
            .and_then(|attrs| attrs.unit.as_deref())
    }

    #[setter]
    fn set_label(&mut self, val: Option<String>) {
        match self.instruction.extra_attrs.as_mut() {
            Some(attrs) => attrs.label = val,
            None => {
                if val.is_some() {
                    self.instruction.extra_attrs = Some(Box::new(
                        crate::circuit_instruction::ExtraInstructionAttributes {
                            label: val,
                            duration: None,
                            unit: None,
                            condition: None,
                        },
                    ))
                }
            }
        };
        if let Some(attrs) = &self.instruction.extra_attrs {
            if attrs.label.is_none()
                && attrs.duration.is_none()
                && attrs.unit.is_none()
                && attrs.condition.is_none()
            {
                self.instruction.extra_attrs = None;
            }
        }
    }

    #[getter]
    fn definition<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        let definition = self
            .instruction
            .operation
            .definition(&self.instruction.params);
        definition
            .map(|data| {
                QUANTUM_CIRCUIT
                    .get_bound(py)
                    .call_method1(intern!(py, "_from_circuit_data"), (data,))
            })
            .transpose()
    }

    /// Sets the Instruction name corresponding to the op for this node
    #[setter]
    fn set_name(&mut self, py: Python, new_name: PyObject) -> PyResult<()> {
        let op = operation_type_to_py(py, &self.instruction)?;
        op.bind(py).setattr(intern!(py, "name"), new_name)?;
        let res = convert_py_to_operation_type(py, op)?;
        self.instruction.operation = res.operation;
        Ok(())
    }

    #[getter]
    fn _raw_op(&self, py: Python) -> PyObject {
        self.instruction.operation.clone().into_py(py)
    }

    /// Returns a representation of the DAGOpNode
    fn __repr__(&self, py: Python) -> PyResult<String> {
        Ok(format!(
            "DAGOpNode(op={}, qargs={}, cargs={})",
            operation_type_to_py(py, &self.instruction)?
                .bind(py)
                .repr()?,
            self.instruction.qubits.bind(py).repr()?,
            self.instruction.clbits.bind(py).repr()?
        ))
    }
}

/// Object to represent an incoming wire node in the DAGCircuit.
#[pyclass(module = "qiskit._accelerate.circuit", extends=DAGNode)]
pub struct DAGInNode {
    #[pyo3(get)]
    wire: PyObject,
    #[pyo3(get)]
    sort_key: PyObject,
}

#[pymethods]
impl DAGInNode {
    #[new]
    fn new(py: Python, wire: PyObject) -> PyResult<(Self, DAGNode)> {
        Ok((
            DAGInNode {
                wire,
                sort_key: PyList::empty_bound(py).str()?.into_any().unbind(),
            },
            DAGNode { _node_id: -1 },
        ))
    }

    fn __reduce__(slf: PyRef<Self>, py: Python) -> PyObject {
        let state = (slf.as_ref()._node_id, &slf.sort_key);
        (py.get_type_bound::<Self>(), (&slf.wire,), state).into_py(py)
    }

    fn __setstate__(mut slf: PyRefMut<Self>, state: &Bound<PyAny>) -> PyResult<()> {
        let (nid, sort_key): (isize, PyObject) = state.extract()?;
        slf.as_mut()._node_id = nid;
        slf.sort_key = sort_key;
        Ok(())
    }

    /// Returns a representation of the DAGInNode
    fn __repr__(&self, py: Python) -> PyResult<String> {
        Ok(format!("DAGInNode(wire={})", self.wire.bind(py).repr()?))
    }
}

/// Object to represent an outgoing wire node in the DAGCircuit.
#[pyclass(module = "qiskit._accelerate.circuit", extends=DAGNode)]
pub struct DAGOutNode {
    #[pyo3(get)]
    wire: PyObject,
    #[pyo3(get)]
    sort_key: PyObject,
}

#[pymethods]
impl DAGOutNode {
    #[new]
    fn new(py: Python, wire: PyObject) -> PyResult<(Self, DAGNode)> {
        Ok((
            DAGOutNode {
                wire,
                sort_key: PyList::empty_bound(py).str()?.into_any().unbind(),
            },
            DAGNode { _node_id: -1 },
        ))
    }

    fn __reduce__(slf: PyRef<Self>, py: Python) -> PyObject {
        let state = (slf.as_ref()._node_id, &slf.sort_key);
        (py.get_type_bound::<Self>(), (&slf.wire,), state).into_py(py)
    }

    fn __setstate__(mut slf: PyRefMut<Self>, state: &Bound<PyAny>) -> PyResult<()> {
        let (nid, sort_key): (isize, PyObject) = state.extract()?;
        slf.as_mut()._node_id = nid;
        slf.sort_key = sort_key;
        Ok(())
    }

    /// Returns a representation of the DAGOutNode
    fn __repr__(&self, py: Python) -> PyResult<String> {
        Ok(format!("DAGOutNode(wire={})", self.wire.bind(py).repr()?))
    }
}
