// Copyright (C) 2019-2022 Aleo Systems Inc.
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

use crate::{
    function::instruction::Operand,
    program::{Program, RegisterType, Stack, StackValue},
    Balance,
    Entry,
    EntryType,
    Literal,
    Opcode,
    Owner,
    Plaintext,
    PlaintextType,
    Record,
    Register,
    ValueType,
};
use snarkvm_console_network::prelude::*;

use indexmap::IndexMap;

/// Casts the operands into the declared type.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Cast<N: Network> {
    /// The operands.
    operands: Vec<Operand<N>>,
    /// The destination register.
    destination: Register<N>,
    /// The casted value type.
    value_type: ValueType<N>,
}

impl<N: Network> Cast<N> {
    /// Returns the opcode.
    #[inline]
    pub const fn opcode() -> Opcode {
        Opcode::Cast
    }

    /// Returns the operands in the operation.
    #[inline]
    pub fn operands(&self) -> &[Operand<N>] {
        &self.operands
    }

    /// Returns the destination register.
    #[inline]
    pub const fn destination(&self) -> &Register<N> {
        &self.destination
    }

    /// Returns the casted value type.
    #[inline]
    pub const fn value_type(&self) -> &ValueType<N> {
        &self.value_type
    }
}

impl<N: Network> Cast<N> {
    /// Evaluates the instruction.
    #[inline]
    pub(in crate::function) fn evaluate(&self, stack: &mut Stack<N>) -> Result<()> {
        // Initialize a vector to store the operand literals.
        let mut inputs = Vec::with_capacity(self.operands.len());

        // Load the operands **as literals & literal types**.
        self.operands.iter().try_for_each(|operand| {
            // Load and append the value.
            inputs.push(stack.load(operand)?);
            // Move to the next iteration.
            Ok::<_, Error>(())
        })?;

        match self.value_type {
            ValueType::Constant(plaintext_type)
            | ValueType::Public(plaintext_type)
            | ValueType::Private(plaintext_type) => {
                match plaintext_type {
                    PlaintextType::Literal(..) => bail!("Casting to literal is currently unsupported"),
                    PlaintextType::Interface(interface_name) => {
                        // Ensure the inputs is not empty.
                        ensure!(!inputs.is_empty(), "Casting to an interface requires at least one input");

                        // Retrieve the interface and ensure it is defined in the program.
                        let interface = stack.get_interface(&interface_name)?;

                        // Initialize the interface members.
                        let mut members = IndexMap::new();
                        for (member, (member_name, member_type)) in inputs.iter().zip_eq(interface.members()) {
                            // Compute the register type.
                            let register_type = RegisterType::Plaintext(*member_type);
                            // Retrieve the plaintext value from the entry.
                            let plaintext = match member {
                                StackValue::Plaintext(plaintext) => {
                                    // Ensure the member matches the register type.
                                    stack
                                        .matches_register(&StackValue::Plaintext(plaintext.clone()), &register_type)?;
                                    // Output the plaintext.
                                    plaintext.clone()
                                }
                                // Ensure the interface member is not a record.
                                StackValue::Record(..) => bail!("Casting a record into an interface member is illegal"),
                            };
                            // Append the member to the interface members.
                            members.insert(*member_name, plaintext);
                        }

                        // Construct the interface.
                        let interface = Plaintext::Interface(members, Default::default());
                        // Store the interface.
                        stack.store(&self.destination, StackValue::Plaintext(interface))
                    }
                }
            }
            ValueType::Record(record_name) => {
                // Ensure the inputs length is at least 2.
                ensure!(inputs.len() >= 2, "Casting to record requires at least two inputs");

                // Retrieve the interface and ensure it is defined in the program.
                let record_type = stack.get_record(&record_name)?;

                // Initialize the record owner.
                let owner: Owner<N, Plaintext<N>> = match &inputs[0] {
                    // Ensure the entry is an address.
                    StackValue::Plaintext(Plaintext::Literal(Literal::Address(owner), ..)) => {
                        match record_type.owner().is_public() {
                            true => Owner::Public(owner.clone()),
                            false => {
                                Owner::Private(Plaintext::Literal(Literal::Address(owner.clone()), Default::default()))
                            }
                        }
                    }
                    _ => bail!("Invalid record owner"),
                };

                // Initialize the record balance.
                let balance: Balance<N, Plaintext<N>> = match &inputs[1] {
                    // Ensure the entry is an balance.
                    StackValue::Plaintext(Plaintext::Literal(Literal::U64(balance), ..)) => {
                        // Ensure the balance is less than or equal to 2^52.
                        ensure!(
                            balance.to_bits_le()[52..].iter().all(|bit| !bit),
                            "Attempted to initialize an invalid balance"
                        );
                        // Construct the record balance.
                        match record_type.balance().is_public() {
                            true => Balance::Public(balance.clone()),
                            false => {
                                Balance::Private(Plaintext::Literal(Literal::U64(balance.clone()), Default::default()))
                            }
                        }
                    }
                    _ => bail!("Invalid record balance"),
                };

                // Initialize the record entries.
                let mut entries = IndexMap::new();
                for (entry, (entry_name, entry_type)) in inputs.iter().zip_eq(record_type.entries()) {
                    // Compute the register type.
                    let register_type = RegisterType::from(ValueType::from(*entry_type));
                    // Retrieve the plaintext value from the entry.
                    let plaintext = match entry {
                        StackValue::Plaintext(plaintext) => {
                            // Ensure the entry matches the register type.
                            stack.matches_register(&StackValue::Plaintext(plaintext.clone()), &register_type)?;
                            // Output the plaintext.
                            plaintext.clone()
                        }
                        // Ensure the record entry is not a record.
                        StackValue::Record(..) => bail!("Casting a record into a record entry is illegal"),
                    };
                    // Append the entry to the record entries.
                    match entry_type {
                        EntryType::Constant(..) => entries.insert(*entry_name, Entry::Constant(plaintext)),
                        EntryType::Public(..) => entries.insert(*entry_name, Entry::Public(plaintext)),
                        EntryType::Private(..) => entries.insert(*entry_name, Entry::Private(plaintext)),
                    };
                }

                // Construct the record.
                let record = Record::new(owner, balance, entries)?;
                // Store the record.
                stack.store(&self.destination, StackValue::Record(record))
            }
        }
    }

    /// Returns the output type from the given program and input types.
    #[inline]
    pub fn output_type(&self, program: &Program<N>, input_types: &[RegisterType<N>]) -> Result<RegisterType<N>> {
        // Ensure the number of operands is correct.
        ensure!(
            input_types.len() == self.operands.len(),
            "Instruction '{}' expects {} operands, found {} operands",
            Self::opcode(),
            input_types.len(),
            self.operands.len(),
        );

        // Ensure the output type is defined in the program.
        match self.value_type {
            ValueType::Constant(plaintext_type)
            | ValueType::Public(plaintext_type)
            | ValueType::Private(plaintext_type) => {
                match plaintext_type {
                    PlaintextType::Literal(..) => bail!("Casting to literal is currently unsupported"),
                    PlaintextType::Interface(interface_name) => {
                        // Retrieve the interface and ensure it is defined in the program.
                        let interface = program.get_interface(&interface_name)?;
                        // Ensure the input types match the interface.
                        for ((_, member_type), input_type) in interface.members().iter().zip_eq(input_types) {
                            match input_type {
                                // Ensure the plaintext type matches the member type.
                                RegisterType::Plaintext(plaintext_type) => {
                                    ensure!(
                                        member_type == plaintext_type,
                                        "Interface '{interface_name}' member type mismatch: expected '{member_type}', found '{plaintext_type}'"
                                    )
                                }
                                // Ensure the input type cannot be a record (this is unsupported behavior).
                                RegisterType::Record(record_name) => bail!(
                                    "Interface '{interface_name}' member type mismatch: expected '{member_type}', found record '{record_name}'"
                                ),
                            }
                        }
                    }
                }
            }
            ValueType::Record(record_name) => {
                // Retrieve the record type and ensure is defined in the program.
                let record = program.get_record(&record_name)?;
                // Ensure the input types match the record.
                for ((_, entry_type), input_type) in record.entries().iter().zip_eq(input_types) {
                    match input_type {
                        // Ensure the plaintext type matches the entry type.
                        RegisterType::Plaintext(plaintext_type) => match entry_type {
                            EntryType::Constant(entry_type)
                            | EntryType::Public(entry_type)
                            | EntryType::Private(entry_type) => {
                                ensure!(
                                    entry_type == plaintext_type,
                                    "Record '{record_name}' entry type mismatch: expected '{entry_type}', found '{plaintext_type}'"
                                )
                            }
                        },
                        // Ensure the input type cannot be a record (this is unsupported behavior).
                        RegisterType::Record(record_name) => bail!(
                            "Record '{record_name}' entry type mismatch: expected '{entry_type}', found record '{record_name}'"
                        ),
                    }
                }
            }
        }

        Ok(RegisterType::from(self.value_type.clone()))
    }
}

impl<N: Network> Parser for Cast<N> {
    /// Parses a string into an operation.
    #[inline]
    fn parse(string: &str) -> ParserResult<Self> {
        /// Parses an operand from the string.
        fn parse_operand<N: Network>(string: &str) -> ParserResult<Operand<N>> {
            // Parse the whitespace from the string.
            let (string, _) = Sanitizer::parse_whitespaces(string)?;
            // Parse the operand from the string.
            Operand::parse(string)
        }

        // Parse the opcode from the string.
        let (string, _) = tag(*Self::opcode())(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the operands from the string.
        let (string, operands) = map_res(many1(parse_operand), |operands: Vec<Operand<N>>| {
            // Ensure the number of operands is within the bounds.
            match operands.len() <= N::MAX_OPERANDS {
                true => Ok(operands),
                false => Err(error("Failed to parse 'cast' opcode: too many operands")),
            }
        })(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the "into" from the string.
        let (string, _) = tag("into")(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the destination register from the string.
        let (string, destination) = Register::parse(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the "as" from the string.
        let (string, _) = tag("as")(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the value type from the string.
        let (string, value_type) = ValueType::parse(string)?;

        Ok((string, Self { operands, destination, value_type }))
    }
}

impl<N: Network> FromStr for Cast<N> {
    type Err = Error;

    /// Parses a string into an operation.
    #[inline]
    fn from_str(string: &str) -> Result<Self> {
        match Self::parse(string) {
            Ok((remainder, object)) => {
                // Ensure the remainder is empty.
                ensure!(remainder.is_empty(), "Failed to parse string. Found invalid character in: \"{remainder}\"");
                // Return the object.
                Ok(object)
            }
            Err(error) => bail!("Failed to parse string. {error}"),
        }
    }
}

impl<N: Network> Debug for Cast<N> {
    /// Prints the operation as a string.
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl<N: Network> Display for Cast<N> {
    /// Prints the operation to a string.
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        // Ensure the number of operands is within the bounds.
        if self.operands.len().is_zero() || self.operands.len() > N::MAX_OPERANDS {
            eprintln!("The number of operands must be nonzero and <= {}", N::MAX_OPERANDS);
            return Err(fmt::Error);
        }
        // Print the operation.
        write!(f, "{} ", Self::opcode())?;
        self.operands.iter().try_for_each(|operand| write!(f, "{} ", operand))?;
        write!(f, "into {} as {}", self.destination, self.value_type)
    }
}

impl<N: Network> FromBytes for Cast<N> {
    /// Reads the operation from a buffer.
    fn read_le<R: Read>(mut reader: R) -> IoResult<Self> {
        // Read the number of operands.
        let num_operands = u8::read_le(&mut reader)? as usize;

        // Ensure the number of operands is within the bounds.
        if num_operands.is_zero() || num_operands > N::MAX_OPERANDS {
            return Err(error(format!("The number of operands must be nonzero and <= {}", N::MAX_OPERANDS)));
        }

        // Initialize the vector for the operands.
        let mut operands = Vec::with_capacity(num_operands);
        // Read the operands.
        for _ in 0..num_operands {
            operands.push(Operand::read_le(&mut reader)?);
        }

        // Read the destination register.
        let destination = Register::read_le(&mut reader)?;

        // Read the casted value type.
        let value_type = ValueType::read_le(&mut reader)?;

        // Return the operation.
        Ok(Self { operands, destination, value_type })
    }
}

impl<N: Network> ToBytes for Cast<N> {
    /// Writes the operation to a buffer.
    fn write_le<W: Write>(&self, mut writer: W) -> IoResult<()> {
        // Ensure the number of operands is within the bounds.
        if self.operands.len().is_zero() || self.operands.len() > N::MAX_OPERANDS {
            return Err(error(format!("The number of operands must be nonzero and <= {}", N::MAX_OPERANDS)));
        }

        // Write the number of operands.
        (self.operands.len() as u8).write_le(&mut writer)?;
        // Write the operands.
        self.operands.iter().try_for_each(|operand| operand.write_le(&mut writer))?;
        // Write the destination register.
        self.destination.write_le(&mut writer)?;
        // Write the casted value type.
        self.value_type.write_le(&mut writer)
    }
}
