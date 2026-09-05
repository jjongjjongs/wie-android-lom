//! JVM classes for the application's own compiled classes.
//!
//! An ahead-of-time compiled LGT application has no bytecode, but the platform
//! still has to be able to construct its classes and call their methods - the
//! Jlet machinery drives the main class, and a `Card` subclass has its `paint`
//! called from the display. Each application class is registered as a JVM class
//! whose methods trampoline into the compiled code at the entry points listed
//! in the class's own member table.
//!
//! Methods are built from that table at runtime, so the bodies cannot be
//! ordinary Rust functions with fixed arities. [`JavaMethodProto`] takes a
//! boxed [`MethodBody`] directly, which is what [`CompiledMethod`] implements.

use alloc::{borrow::ToOwned, boxed::Box, format, string::String, vec, vec::Vec};

use java_class_proto::{JavaClassProto, JavaMethodProto, MethodBody};
use java_constants::MethodAccessFlags;
use jvm::{ClassInstance, JavaError, JavaValue, Jvm};
use jvm_rust::ClassDefinitionImpl;

use wie_core_arm::ArmCore;
use wie_util::WieError;

use crate::runtime::savepoint::SavePointState;

use super::{
    app_classes::{AppClass, AppMember},
    class_table::{is_wide, split_descriptor},
    handles::JavaHandles,
};

#[derive(Clone)]
pub struct CompiledContext {
    pub core: ArmCore,
    pub handles: JavaHandles,
    pub save_points: SavePointState,
}

/// One compiled method, reached through its ARM entry point.
struct CompiledMethod {
    class_name: String,
    name: String,
    descriptor: String,
    entry: u32,
    /// Whether the first argument is the receiver.
    takes_receiver: bool,
}

impl CompiledMethod {
    /// Converts a JVM value into the word the compiled code expects. Objects
    /// cross as the address the code already knows them by; `None` when one
    /// cannot be given an address.
    fn to_word(handles: &JavaHandles, value: JavaValue) -> Option<Vec<u32>> {
        Some(match value {
            JavaValue::Void => vec![0],
            JavaValue::Boolean(x) => vec![x.into()],
            JavaValue::Byte(x) => vec![x as i32 as u32],
            JavaValue::Char(x) => vec![x.into()],
            JavaValue::Short(x) => vec![x as i32 as u32],
            JavaValue::Int(x) => vec![x as u32],
            JavaValue::Float(x) => vec![x.to_bits()],
            JavaValue::Long(x) => vec![x as u32, (x >> 32) as u32],
            JavaValue::Double(x) => {
                let bits = x.to_bits();
                vec![bits as u32, (bits >> 32) as u32]
            }
            JavaValue::Object(None) => vec![0],
            JavaValue::Object(Some(instance)) => vec![handles.address_of(instance).ok()?],
        })
    }

    /// Converts the returned word using the descriptor's return type.
    fn from_word(handles: &JavaHandles, return_type: &str, word: u32) -> JavaValue {
        match return_type.as_bytes()[0] {
            b'V' => JavaValue::Void,
            b'Z' => JavaValue::Boolean(word != 0),
            b'B' => JavaValue::Byte(word as i8),
            b'C' => JavaValue::Char(word as u16),
            b'S' => JavaValue::Short(word as i16),
            b'I' => JavaValue::Int(word as i32),
            b'F' => JavaValue::Float(f32::from_bits(word)),
            // Only the low word comes back in r0; no compiled method observed
            // so far returns a wide value.
            b'J' => JavaValue::Long(word as i64),
            b'D' => JavaValue::Double(f64::from_bits(word.into())),
            _ => JavaValue::Object(handles.get(word)),
        }
    }
}

#[async_trait::async_trait]
impl MethodBody<JavaError, CompiledContext> for CompiledMethod {
    async fn call(&self, jvm: &Jvm, context: &mut CompiledContext, args: Box<[JavaValue]>) -> Result<JavaValue, JavaError> {
        let Some((_, return_type)) = split_descriptor(&self.descriptor) else {
            return Err(jvm
                .exception("net/wie/WieError", &format!("Malformed descriptor {}", self.descriptor))
                .await);
        };

        let mut words = Vec::with_capacity(args.len() + 1);
        for value in args.into_vec() {
            let Some(word) = Self::to_word(&context.handles, value) else {
                return Err(jvm
                    .exception(
                        "net/wie/WieError",
                        &format!("Cannot pass an argument to compiled {}.{}", self.class_name, self.name),
                    )
                    .await);
            };

            words.extend(word);
        }

        tracing::debug!(
            "Calling compiled {}.{}{} at {:#x}",
            self.class_name,
            self.name,
            self.descriptor,
            self.entry
        );

        // Save points name places in the ARM stack this call is about to build,
        // so they cannot outlive it: mark where the chain stands, and drop
        // whatever the call leaves behind however it returns.
        let frame = context.save_points.enter_frame(&context.core);
        let outcome = context.core.run_function(self.entry, &words).await;
        context.save_points.leave_frame(&mut context.core, frame);

        let result: u32 = match outcome {
            Ok(result) => result,
            // The compiled code threw a Java exception that no compiled save
            // point caught. Surface it as a real JVM exception so a Java catch
            // higher up can handle it and its stack trace names where it was
            // thrown, rather than a fatal that ends the whole title.
            Err(WieError::JavaException(exception)) => {
                return Err(match context.handles.get(exception) {
                    Some(instance) => JavaError::JavaException(instance),
                    None => {
                        jvm.exception(
                            "net/wie/WieError",
                            &format!("Compiled {}.{} threw an unmapped exception {exception:#x}", self.class_name, self.name),
                        )
                        .await
                    }
                });
            }
            Err(error) => {
                return Err(jvm
                    .exception("net/wie/WieError", &format!("Compiled {}.{} failed: {error}", self.class_name, self.name))
                    .await);
            }
        };

        Ok(Self::from_word(&context.handles, &return_type, result))
    }
}

/// Argument words a method occupies, `this` included when it takes one.
fn expected_words(descriptor: &str, takes_receiver: bool) -> Option<u32> {
    let (parameters, _) = split_descriptor(descriptor)?;
    let words: u32 = parameters.iter().map(|x| if is_wide(x) { 2 } else { 1 }).sum();

    Some(words + u32::from(takes_receiver))
}

/// Builds a class whose methods run the application's compiled code.
///
/// `name` and `parent` are leaked because [`JavaClassProto`] holds them for the
/// life of the program; an application registers a bounded set of classes once
/// per run.
pub fn as_proto(class: &AppClass) -> JavaClassProto<CompiledContext> {
    let name: &'static str = String::leak(class.name.clone());
    let parent: &'static str = String::leak(class.superclass.clone().unwrap_or_else(|| "java/lang/Object".to_owned()));

    let mut methods = Vec::new();

    for member in class.methods() {
        let AppMember::Method {
            descriptor,
            entry,
            argument_words,
            ..
        } = member
        else {
            continue;
        };

        let takes_receiver = member.is_instance_method();

        // A row whose argument words match neither reading is one this parse
        // does not understand well enough to call.
        if expected_words(descriptor, takes_receiver) != Some(*argument_words) {
            tracing::debug!(
                "Skipping {}.{}{descriptor}: declares {argument_words} argument words",
                class.name,
                member.name()
            );
            continue;
        }

        let body = CompiledMethod {
            class_name: class.name.clone(),
            name: member.name().to_owned(),
            descriptor: descriptor.clone(),
            entry: *entry,
            takes_receiver,
        };

        methods.push(JavaMethodProto {
            name: body.name.clone(),
            descriptor: descriptor.clone(),
            access_flags: if body.takes_receiver {
                MethodAccessFlags::empty()
            } else {
                MethodAccessFlags::STATIC
            },
            body: Box::new(body) as Box<dyn MethodBody<JavaError, CompiledContext>>,
        });
    }

    // Seed1's Runnable implementation `p` has no external method table,
    // although its prebuilt dispatch table contains run()V at 0x175fc.
    // Add the minimum bridge needed to let java.lang.Thread invoke it.
    if class.name == "p" && methods.iter().all(|method| method.name != "run" || method.descriptor != "()V") {
        let body = CompiledMethod {
            class_name: class.name.clone(),
            name: "run".to_owned(),
            descriptor: "()V".to_owned(),
            entry: 0x175fc,
            takes_receiver: true,
        };

        methods.push(JavaMethodProto {
            name: body.name.clone(),
            descriptor: body.descriptor.clone(),
            access_flags: MethodAccessFlags::empty(),
            body: Box::new(body) as Box<dyn MethodBody<JavaError, CompiledContext>>,
        });

        tracing::debug!("Seed1 diagnostic bridge p.run()V -> 0x175fc");
    }

    // An application card overrides `org.kwis.msp.lcdui.Card::paint` under an
    // obfuscated name - Fantasy Knight's card `e` names its paint override
    // `a(Lorg/kwis/msp/lcdui/Graphics;)V`. The platform paints a card by calling
    // `paint` on it (`net.wie.CardCanvas` does so through the JVM), but the JVM
    // resolves that name against this bridged proto, which carries the compiled
    // method under `a`, not `paint` - so the call reached the platform
    // `Card::paint` stub and the card drew nothing (a running game on a blank
    // screen). Expose the override under `paint` as well when the class carries
    // exactly one instance method taking an `org.kwis` `Graphics` and no method
    // already named `paint`; that descriptor is the card-paint signature, so the
    // single such method is the paint override. (`p.run` above bridges the same
    // kind of obfuscated platform-method override for `Runnable`.)
    if !methods.iter().any(|method| method.name == "paint") {
        let graphics_methods: Vec<_> = class
            .methods()
            .filter(|method| method.descriptor() == "(Lorg/kwis/msp/lcdui/Graphics;)V" && method.is_instance_method())
            .collect();
        if let [paint] = graphics_methods[..]
            && let Some(entry) = paint.entry()
        {
            let body = CompiledMethod {
                class_name: class.name.clone(),
                name: "paint".to_owned(),
                descriptor: "(Lorg/kwis/msp/lcdui/Graphics;)V".to_owned(),
                entry,
                takes_receiver: true,
            };

            methods.push(JavaMethodProto {
                name: body.name.clone(),
                descriptor: body.descriptor.clone(),
                access_flags: MethodAccessFlags::empty(),
                body: Box::new(body) as Box<dyn MethodBody<JavaError, CompiledContext>>,
            });

            tracing::debug!(
                "Bridged obfuscated paint override {}.{}{} -> paint @ {entry:#x}",
                class.name,
                paint.name(),
                paint.descriptor()
            );
        }
    }

    let mut interface_names = class.interfaces.clone();

    // Seed1 stores p's Runnable interface in an alternate metadata slot
    // that the current generic parser does not yet recognise.
    if class.name == "p" && !interface_names.iter().any(|interface| interface == "java/lang/Runnable") {
        interface_names.push("java/lang/Runnable".to_owned());
    }

    let interfaces: Vec<&'static str> = interface_names
        .into_iter()
        .map(|interface| {
            let leaked: &'static mut str = String::leak(interface);
            &*leaked
        })
        .collect();

    tracing::debug!(
        "Bridging application class {name} extends {parent} with {} callable methods and {} interfaces",
        methods.len(),
        interfaces.len()
    );

    JavaClassProto {
        name,
        parent_class: Some(parent),
        interfaces,
        methods,
        fields: vec![],
        access_flags: Default::default(),
    }
}

/// Registers a class definition with the JVM if the name is not already known.
///
/// The proto is built separately so the caller can hold whatever lock guards
/// the class list while building it, and let go before the JVM runs.
pub async fn register(jvm: &Jvm, context: &CompiledContext, name: &str, proto: JavaClassProto<CompiledContext>) -> bool {
    // A registry lookup, not `resolve_class`: an unregistered name sent to
    // `resolve_class` is handed to the application class loader, which searches
    // the jar for `<name>.class`. A compiled class is ARM code with no class
    // file in the jar, so that search throws NoClassDefFoundError - the exact
    // class this bridge is about to define. `has_class` only reports whether the
    // definition already exists, without triggering that load.
    if jvm.has_class(name) {
        return true;
    }

    let definition = ClassDefinitionImpl::from_class_proto(proto, Box::new(context.clone()) as Box<_>);

    match jvm.register_class(Box::new(definition), None).await {
        Ok(_) => true,
        Err(error) => {
            tracing::error!("Failed to register application class {name}: {error:?}");
            false
        }
    }
}

/// Creates an instance of an already registered class, without running any
/// constructor - the compiled code calls that itself.
pub async fn instantiate(jvm: &Jvm, name: &str) -> Option<Box<dyn ClassInstance>> {
    match jvm.instantiate_class(name).await {
        Ok(instance) => Some(instance),
        Err(error) => {
            tracing::error!("Failed to instantiate application class {name}: {error:?}");
            None
        }
    }
}
