//! Context processing algorithm and related types.

mod definition;
pub mod inverse;
mod loader;
mod processing;

use crate::{
	lang::{LenientLanguageTag, LenientLanguageTagBuf},
	syntax::Term,
	util::{AsJson, JsonFrom},
	Direction, Error, Id, Loc, ProcessingMode, Warning,
};
use futures::{future::BoxFuture, FutureExt};
use generic_json::{JsonClone, JsonSendSync};
use iref::{Iri, IriBuf};
// use langtag::{LanguageTag, LanguageTagBuf};
use std::collections::HashMap;

pub use definition::*;
pub use inverse::{InverseContext, Inversible};
pub use loader::*;
use processing::*;

pub trait JsonContext = JsonSendSync + JsonClone;

/// Options of the Context Processing Algorithm.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProcessingOptions {
	/// The processing mode
	pub processing_mode: ProcessingMode,

	/// Override protected definitions.
	pub override_protected: bool,

	/// Propagate the processed context.
	pub propagate: bool,
}

impl ProcessingOptions {
	/// Return the same set of options, but with `override_protected` set to `true`.
	#[must_use]
	pub fn with_override(&self) -> ProcessingOptions {
		let mut opt = *self;
		opt.override_protected = true;
		opt
	}

	/// Return the same set of options, but with `override_protected` set to `false`.
	#[must_use]
	pub fn with_no_override(&self) -> ProcessingOptions {
		let mut opt = *self;
		opt.override_protected = false;
		opt
	}

	/// Return the same set of options, but with `propagate` set to `false`.
	#[must_use]
	pub fn without_propagation(&self) -> ProcessingOptions {
		let mut opt = *self;
		opt.propagate = false;
		opt
	}
}

impl Default for ProcessingOptions {
	fn default() -> ProcessingOptions {
		ProcessingOptions {
			processing_mode: ProcessingMode::default(),
			override_protected: false,
			propagate: true,
		}
	}
}

/// JSON-LD context.
///
/// A context holds all the term definitions used to expand a JSON-LD value.
pub trait Context<T: Id = IriBuf>: Clone {
	// TODO Later
	// type Definitions<'a>: Iterator<Item = (&'a str, TermDefinition<T, Self>)>;

	/// The type of local contexts associated to this type of contexts.
	type LocalContext: Local<T>;

	/// Create a newly-initialized active context with the given *base IRI*.
	fn new(base_iri: Option<Iri>) -> Self;

	/// Get the definition of a term.
	fn get(&self, term: &str) -> Option<&TermDefinition<T, Self>>;

	fn get_opt(&self, term: Option<&str>) -> Option<&TermDefinition<T, Self>> {
		if let Some(term) = term {
			self.get(term)
		} else {
			None
		}
	}

	#[inline]
	fn contains(&self, term: &str) -> bool {
		self.get(term).is_some()
	}

	/// Original base URL of the context.
	fn original_base_url(&self) -> Option<Iri>;

	/// Current *base IRI* of the context.
	fn base_iri(&self) -> Option<Iri>;

	/// Optional vocabulary mapping.
	fn vocabulary(&self) -> Option<&Term<T>>;

	/// Optional default language.
	fn default_language(&self) -> Option<LenientLanguageTag>;

	/// Optional default base direction.
	fn default_base_direction(&self) -> Option<Direction>;

	/// Get the previous context.
	fn previous_context(&self) -> Option<&Self>;

	fn definitions<'a>(
		&'a self,
	) -> Box<dyn 'a + Iterator<Item = (&'a String, &'a TermDefinition<T, Self>)>>;
}

/// Mutable JSON-LD context.
pub trait ContextMut<T: Id = IriBuf>: Context<T> {
	/// Defines the given term.
	fn set(
		&mut self,
		term: &str,
		definition: Option<TermDefinition<T, Self>>,
	) -> Option<TermDefinition<T, Self>>;

	/// Sets the base IRI of the context.
	fn set_base_iri(&mut self, iri: Option<Iri>);

	/// Sets the vocabulary.
	fn set_vocabulary(&mut self, vocab: Option<Term<T>>);

	/// Sets the default language.
	fn set_default_language(&mut self, lang: Option<LenientLanguageTagBuf>);

	/// Sets de default language base direction.
	fn set_default_base_direction(&mut self, dir: Option<Direction>);

	/// Sets the previous context.
	fn set_previous_context(&mut self, previous: Self);
}

/// Trait for types that are or wrap a mutable context.
///
/// This trait is used by the [`Document::compact`](crate::Document::compact)
/// function to accept either a context or a wrapper to a context.
pub trait ContextMutProxy<T: Id = IriBuf> {
	type Target: ContextMut<T>;

	/// Returns a reference to the mutable context.
	fn deref(&self) -> &Self::Target;
}

/// Context processing result.
pub type ProcessingResult<'s, J, C> =
	Result<Processed<'s, J, C>, Loc<Error, <J as generic_json::Json>::MetaData>>;

/// Local context used for context expansion.
///
/// Local contexts can be seen as "abstract contexts" that can be processed to enrich an
/// existing active context.
pub trait Local<T: Id = IriBuf>: JsonSendSync {
	/// Process the local context with specific options.
	fn process_full<'a, 's: 'a, C: ContextMut<T> + Send + Sync, L: Loader + Send + Sync>(
		&'s self,
		active_context: &'a C,
		stack: ProcessingStack,
		loader: &'a mut L,
		base_url: Option<Iri<'a>>,
		options: ProcessingOptions,
	) -> BoxFuture<'a, ProcessingResult<'s, Self, C>>
	where
		C::LocalContext: From<L::Output> + From<Self>,
		L::Output: Into<Self>,
		T: Send + Sync;

	/// Process the local context with specific options.
	fn process_with<'a, 's: 'a, C: ContextMut<T> + Send + Sync, L: Loader + Send + Sync>(
		&'s self,
		active_context: &'a C,
		loader: &'a mut L,
		base_url: Option<Iri<'a>>,
		options: ProcessingOptions,
	) -> BoxFuture<'a, ProcessingResult<'s, Self, C>>
	where
		C::LocalContext: From<L::Output> + From<Self>,
		L::Output: Into<Self>,
		T: Send + Sync,
	{
		self.process_full(
			active_context,
			ProcessingStack::new(),
			loader,
			base_url,
			options,
		)
	}

	/// Process the local context with the given initial active context with the default options:
	/// `is_remote` is `false`, `override_protected` is `false` and `propagate` is `true`.
	fn process<'a, 's: 'a, C: ContextMut<T> + Default + Send + Sync, L: Loader + Send + Sync>(
		&'s self,
		loader: &'a mut L,
		base_url: Option<Iri<'a>>,
	) -> BoxFuture<'a, ProcessingResult<'s, Self, C>>
	where
		C::LocalContext: From<L::Output> + From<Self>,
		L::Output: Into<Self>,
		T: Send + Sync,
	{
		async move {
			let active_context = C::default();
			self.process_full(
				&active_context,
				ProcessingStack::new(),
				loader,
				base_url,
				ProcessingOptions::default(),
			)
			.await
		}
		.boxed()
	}
}

/// Processed context attached to its original unprocessed local context.
///
/// This is usefull for instance to attach a processed context to its original JSON form,
/// which is then used by the compaction algorithm to put the context in the compacted document.
#[derive(Clone)]
pub struct ProcessedOwned<L: generic_json::Json, C> {
	/// Original unprocessed context.
	local: L,

	/// Processed context.
	processed: C,

	/// Warnings collected during processing.
	warnings: Vec<Loc<Warning, L::MetaData>>,
}

impl<L: generic_json::Json, C> ProcessedOwned<L, C> {
	/// Wraps a processed context along with its original local representation.
	pub fn new(local: L, processed: C) -> ProcessedOwned<L, C> {
		Self::with_warnings(local, processed, Vec::new())
	}

	/// Wraps a processed context along with its original local representation and warnings emitted during processing.
	pub fn with_warnings(local: L, processed: C, warnings: Vec<Loc<Warning, L::MetaData>>) -> Self {
		ProcessedOwned {
			local,
			processed,
			warnings,
		}
	}

	/// Returns a reference to the warnings emitted during processing.
	pub fn warnings(&self) -> &[Loc<Warning, L::MetaData>] {
		&self.warnings
	}

	/// Consumes the wrapper and returns the processed context.
	pub fn into_inner(self) -> C {
		self.processed
	}
}

impl<T: Id, L: generic_json::Json, C: ContextMut<T>> ContextMutProxy<T> for ProcessedOwned<L, C> {
	type Target = C;

	fn deref(&self) -> &C {
		&self.processed
	}
}

impl<L: generic_json::Json, C> std::ops::Deref for ProcessedOwned<L, C> {
	type Target = C;

	fn deref(&self) -> &C {
		&self.processed
	}
}

impl<L: generic_json::Json, C> std::convert::AsRef<C> for ProcessedOwned<L, C> {
	fn as_ref(&self) -> &C {
		&self.processed
	}
}

impl<J: JsonClone, K: JsonFrom<J>, L: generic_json::Json + AsJson<J, K>, C> AsJson<J, K>
	for ProcessedOwned<L, C>
{
	fn as_json_with(&self, meta: impl Clone + Fn(Option<&J::MetaData>) -> <K as generic_json::Json>::MetaData) -> K {
		self.local.as_json_with(meta)
	}
}

/// Processed context referencing its original unprocessed local context.
///
/// This is usefull for instance to attach a processed context to its original JSON form,
/// which is then used by the compaction algorithm to put the context in the compacted document.
#[derive(Clone)]
pub struct Processed<'a, L: generic_json::Json, C> {
	/// Original unprocessed context.
	local: &'a L,

	/// Processed context.
	processed: C,

	/// Warnings collected during processing.
	warnings: Vec<Loc<Warning, L::MetaData>>,
}

impl<'a, L: generic_json::Json, C> Processed<'a, L, C> {
	/// Wraps a processed context along with a reference to its original local representation.
	pub fn new(local: &'a L, processed: C) -> Self {
		Self::with_warnings(local, processed, Vec::new())
	}

	/// Wraps a processed context along with a reference to its original local representation
	/// and warnings emitted during processing.
	pub fn with_warnings(
		local: &'a L,
		processed: C,
		warnings: Vec<Loc<Warning, L::MetaData>>,
	) -> Self {
		Processed {
			local,
			processed,
			warnings,
		}
	}

	/// Returns a reference to the warnings emitted during processing.
	pub fn warnings(&self) -> &[Loc<Warning, L::MetaData>] {
		&self.warnings
	}

	/// Consumes the wrapper and returns the processed context.
	pub fn into_inner(self) -> C {
		self.processed
	}

	/// Clone the referenced local context and return
	/// a `Processed` context that owns the original local context.
	pub fn owned(self) -> ProcessedOwned<L, C>
	where
		L: Clone,
	{
		ProcessedOwned {
			local: L::clone(self.local),
			processed: self.processed,
			warnings: self.warnings,
		}
	}
}

impl<'a, T: Id, L: generic_json::Json, C: ContextMut<T>> ContextMutProxy<T>
	for Processed<'a, L, C>
{
	type Target = C;

	fn deref(&self) -> &C {
		&self.processed
	}
}

impl<'a, L: generic_json::Json, C> std::ops::Deref for Processed<'a, L, C> {
	type Target = C;

	fn deref(&self) -> &C {
		&self.processed
	}
}

impl<'a, L: generic_json::Json, C> std::convert::AsRef<C> for Processed<'a, L, C> {
	fn as_ref(&self) -> &C {
		&self.processed
	}
}

impl<'a, J: JsonClone, K: JsonFrom<J>, L: generic_json::Json + AsJson<J, K>, C> AsJson<J, K>
	for Processed<'a, L, C>
{
	fn as_json_with(&self, meta: impl Clone + Fn(Option<&J::MetaData>) -> <K as generic_json::Json>::MetaData) -> K {
		self.local.as_json_with(meta)
	}
}

#[derive(Clone, PartialEq, Eq)]
pub struct Json<J: JsonContext, T: Id = IriBuf> {
	original_base_url: Option<IriBuf>,
	base_iri: Option<IriBuf>,
	vocabulary: Option<Term<T>>,
	default_language: Option<LenientLanguageTagBuf>,
	default_base_direction: Option<Direction>,
	previous_context: Option<Box<Self>>,
	definitions: HashMap<String, TermDefinition<T, Self>>,
}

impl<J: JsonContext, T: Id> Json<J, T> {
	pub fn new(base_iri: Option<Iri>) -> Self {
		Self {
			original_base_url: base_iri.map(|iri| iri.into()),
			base_iri: base_iri.map(|iri| iri.into()),
			vocabulary: None,
			default_language: None,
			default_base_direction: None,
			previous_context: None,
			definitions: HashMap::new(),
		}
	}
}

impl<J: JsonContext, T: Id> ContextMutProxy<T> for Json<J, T> {
	type Target = Self;

	fn deref(&self) -> &Self {
		self
	}
}

impl<J: JsonContext, T: Id> Default for Json<J, T> {
	fn default() -> Self {
		Self {
			original_base_url: None,
			base_iri: None,
			vocabulary: None,
			default_language: None,
			default_base_direction: None,
			previous_context: None,
			definitions: HashMap::new(),
		}
	}
}

impl<J: JsonContext, T: Id> Context<T> for Json<J, T> {
	type LocalContext = J;

	fn new(base_iri: Option<Iri>) -> Self {
		Self::new(base_iri)
	}

	fn get(&self, term: &str) -> Option<&TermDefinition<T, Self>> {
		self.definitions.get(term)
	}

	fn original_base_url(&self) -> Option<Iri> {
		self.original_base_url.as_ref().map(|iri| iri.as_iri())
	}

	fn base_iri(&self) -> Option<Iri> {
		self.base_iri.as_ref().map(|iri| iri.as_iri())
	}

	fn vocabulary(&self) -> Option<&Term<T>> {
		match &self.vocabulary {
			Some(v) => Some(v),
			None => None,
		}
	}

	fn default_language(&self) -> Option<LenientLanguageTag> {
		self.default_language.as_ref().map(|tag| tag.as_ref())
	}

	fn default_base_direction(&self) -> Option<Direction> {
		self.default_base_direction
	}

	fn previous_context(&self) -> Option<&Self> {
		match &self.previous_context {
			Some(c) => Some(c),
			None => None,
		}
	}

	fn definitions<'a>(
		&'a self,
	) -> Box<dyn 'a + Iterator<Item = (&'a String, &'a TermDefinition<T, Self>)>> {
		Box::new(self.definitions.iter())
	}
}

impl<J: JsonContext, T: Id> ContextMut<T> for Json<J, T> {
	fn set(
		&mut self,
		term: &str,
		definition: Option<TermDefinition<T, Self>>,
	) -> Option<TermDefinition<T, Self>> {
		match definition {
			Some(def) => self.definitions.insert(term.to_string(), def),
			None => self.definitions.remove(term),
		}
	}

	fn set_base_iri(&mut self, iri: Option<Iri>) {
		self.base_iri = match iri {
			Some(iri) => {
				let iri_buf: IriBuf = iri.into();
				Some(iri_buf)
			}
			None => None,
		}
	}

	fn set_vocabulary(&mut self, vocab: Option<Term<T>>) {
		self.vocabulary = vocab;
	}

	fn set_default_language(&mut self, lang: Option<LenientLanguageTagBuf>) {
		self.default_language = lang;
	}

	fn set_default_base_direction(&mut self, dir: Option<Direction>) {
		self.default_base_direction = dir;
	}

	fn set_previous_context(&mut self, previous: Self) {
		self.previous_context = Some(Box::new(previous))
	}
}
