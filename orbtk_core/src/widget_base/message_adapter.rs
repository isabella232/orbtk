use std::{
    any::{Any, TypeId},
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    sync::{mpsc, Arc, Mutex},
};

use crate::shell::WindowRequest;

use dces::entity::Entity;

/// Internal wrapper that stores a message inside a box.
#[derive(Debug)]
pub struct MessageBox {
    message: Box<dyn Any + Send>,
    message_type: TypeId,
    target: Entity,
}

impl MessageBox {
    /// Downcasts the box to a concrete message.
    pub fn downcast<M: Any>(self) -> Result<M, String> {
        if self.message_type == TypeId::of::<M>() {
            return Ok(*self.message.downcast::<M>().unwrap());
        }

        Err("Wrong message type".to_string())
    }

    /// Downcasts the box as reference of a concrete message.
    pub fn downcast_ref<M: Any>(&self) -> Result<&M, String> {
        if self.message_type == TypeId::of::<M>() {
            return Ok(&*self.message.downcast_ref::<M>().unwrap());
        }

        Err("Wrong message type".to_string())
    }

    /// Check if the given type is the type of the message.
    pub fn is_type<M: Any>(&self) -> bool {
        self.message_type == TypeId::of::<M>()
    }

    /// Creates a new `MessageBox`.
    pub fn new<M>(message: M, target: Entity) -> Self
    where
        M: Any + Send,
    {
        MessageBox {
            message: Box::new(message),
            target,
            message_type: TypeId::of::<M>(),
        }
    }

    /// Returns the type of the event.
    pub fn message_type(&self) -> TypeId {
        self.message_type
    }

    /// Returns the target of the event.
    pub fn target(&self) -> Entity {
        self.target
    }
}

/// The `MessageAdapter` provides a thread save entry point to sent
/// and read messages inside widget entities. They are processed inside the
/// method `message` defined in each widgets `State` code.
///
/// # Example
///
/// ```rust
/// // State
/// fn say_hello(entity: Entity, message_adapter: MessageAdapter) {
///     message_adapter.send_message(String::from("Hello rustician"), entity);
///     message_adapter.send_message(String::from("Did you recieve my message?"), entity);
/// }
///
/// impl MyState {}
///
/// // implementation for the State
/// impl State for MyState {
///     fn message(&mut self, mut messages: MessageReader, _registry: &mut Registry, _ctx: &mut Context) {
///         for message in messages.read::<String>() {
///             println!("{}", message);
///         }
///     }
/// }
/// ```
///
/// The example code snippet above will print two lines to stdout:
///
/// ```text
/// $ Hello rustician,
/// $  Did you receive my message?
/// ```
#[derive(Clone, Debug)]
pub struct MessageAdapter {
    messages: Arc<Mutex<BTreeMap<Entity, HashMap<TypeId, Vec<MessageBox>>>>>,
    window_sender: mpsc::Sender<WindowRequest>,
}

impl MessageAdapter {
    /// Creates a new message adapter
    pub fn new(window_sender: mpsc::Sender<WindowRequest>) -> Self {
        MessageAdapter {
            messages: Arc::new(Mutex::new(BTreeMap::new())),
            window_sender,
        }
    }

    /// Send a new message to the message pipeline.
    pub fn send_message<M: Any + Send>(&self, message: M, target: Entity) {
        // Docs say this method must be thread safe
        // to ensure thread safety,
        // do not release this lock until this method execution ends
        let mut locked_messages = self
            .messages
            .lock()
            .expect("MessageAdapter::send_message: Cannot lock messages.");
        locked_messages
            .entry(target)
            .or_insert_with(HashMap::new)
            .entry(TypeId::of::<M>())
            .or_insert_with(Vec::new)
            .push(MessageBox::new(message, target));

        self.window_sender
            .send(WindowRequest::Redraw)
            .expect("MessageAdapter::send_message: Cannot send redraw request.");
    }

    /// Returns a list of entities that has messages.
    pub(crate) fn entities(&self) -> Vec<Entity> {
        self.messages
            .lock()
            .expect("MessageAdapter::entities: Cannot lock messages.")
            .keys()
            .cloned()
            .collect()
    }

    /// Removes all messages for the given target entity. This is used
    /// to remove messages for entities that does not have a `State`
    /// to read the messages.
    pub(crate) fn remove_message_for_entity(&self, target: Entity) {
        self.messages
            .lock()
            .expect("MessageAdapter::remove_message_for_entity: Cannot lock messages.")
            .remove(&target);
    }

    /// Returns the number of messages in the queue.
    pub fn len(&self) -> usize {
        self.messages
            .lock()
            .expect("MessageAdapter::len: Cannot lock messages.")
            .len()
    }

    /// Returns `true` if the event message contains no events.
    pub fn is_empty(&self) -> bool {
        self.messages
            .lock()
            .expect("MessageAdapter::is_empty: Cannot lock messages.")
            .is_empty()
    }

    /// Returns a message reader for the given entity. Moves all
    /// messages for the entity from the adapter to the reader.
    pub(crate) fn message_reader(&self, entity: Entity) -> MessageReader {
        let messages = if let Some(messages) = self
            .messages
            .lock()
            .expect("MessageAdapter::message_reader: Cannot lock messages.")
            .remove(&entity)
        {
            messages
        } else {
            HashMap::new()
        };

        MessageReader::new(messages, entity)
    }
}

/// The `MessageReader` is used to access the messages of a widget.
pub struct MessageReader {
    messages: HashMap<TypeId, Vec<MessageBox>>,
    target: Entity,
}

impl MessageReader {
    /// Creates a new message reader.
    pub fn new(messages: HashMap<TypeId, Vec<MessageBox>>, target: Entity) -> Self {
        MessageReader { messages, target }
    }

    /// Returns the target entity of the reader.
    pub fn entity(&self) -> Entity {
        self.target
    }

    /// Returns `true` if the reader contains message for the specified type.
    pub fn contains_type<M: Any>(&self) -> bool {
        self.messages.contains_key(&TypeId::of::<M>())
    }

    /// Returns `true` if the reader contains no messages.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Returns a message iterator for the given message type.
    pub fn read<M: Any + Send>(&mut self) -> MessageIterator<M> {
        let messages = if let Some(messages) = self.messages.remove(&TypeId::of::<M>()) {
            messages
        } else {
            vec![]
        };

        MessageIterator::new(messages)
    }
}

/// Iterator of messages.
#[derive(Debug)]
pub struct MessageIterator<M>
where
    M: Any + Send,
{
    messages: Vec<MessageBox>,
    _phantom: PhantomData<M>,
}

impl<M> MessageIterator<M>
where
    M: Any + Send,
{
    pub(crate) fn new(messages: Vec<MessageBox>) -> Self {
        MessageIterator {
            messages,
            _phantom: PhantomData::default(),
        }
    }
}

impl<M> Iterator for MessageIterator<M>
where
    M: Any + Send,
{
    type Item = M;

    fn next(&mut self) -> Option<Self::Item> {
        if self.messages.is_empty() {
            return None;
        }

        // unwrap is ok because only messages of the same type should
        // be stored in the vec
        Some(self.messages.remove(0).downcast::<M>().unwrap())
    }
}
