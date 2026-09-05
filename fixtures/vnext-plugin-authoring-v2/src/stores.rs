use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use lenso_capability_document_store::{
    DocumentStoreEndpoint, DocumentStoreProvider, DocumentStorePut, DocumentStoreRead, PutError,
    PutRequest, PutResponse, ReadError, ReadRequest, ReadResponse,
};
use lenso_kernel::{InvocationContext, NativeRequestFuture, RuntimeFailure};
use lenso_native_adapter::{NativePluginFactory, NativePluginFactoryContext, NativePluginInstance};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreCall {
    Read { document: String },
    Put { document: String, text: String },
}

#[derive(Clone, Debug, Default)]
pub struct Account {
    inner: Rc<RefCell<AccountState>>,
}

#[derive(Debug, Default)]
struct AccountState {
    documents: BTreeMap<String, String>,
    calls: Vec<StoreCall>,
}

impl Account {
    pub fn with_document(document: &str, text: &str) -> Self {
        let account = Self::default();
        account
            .inner
            .borrow_mut()
            .documents
            .insert(document.to_owned(), text.to_owned());
        account
    }

    pub fn text(&self, document: &str) -> Option<String> {
        self.inner.borrow().documents.get(document).cloned()
    }

    pub fn calls(&self) -> Vec<StoreCall> {
        self.inner.borrow().calls.clone()
    }
}

#[derive(Debug)]
pub struct StoreFactory {
    package_id: &'static str,
    account: Account,
}

impl StoreFactory {
    pub const fn new(package_id: &'static str, account: Account) -> Self {
        Self {
            package_id,
            account,
        }
    }
}

impl NativePluginFactory for StoreFactory {
    fn package_id(&self) -> &'static str {
        self.package_id
    }

    fn instantiate(
        &self,
        _context: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::new(vec![Rc::new(
            DocumentStoreEndpoint::new(StoreProvider(self.account.clone())),
        )]))
    }
}

#[derive(Debug)]
struct StoreProvider(Account);

impl DocumentStoreProvider for StoreProvider {
    fn put(
        &self,
        _context: InvocationContext,
        request: PutRequest,
    ) -> NativeRequestFuture<DocumentStorePut> {
        let mut account = self.0.inner.borrow_mut();
        account.calls.push(StoreCall::Put {
            document: request.document.clone(),
            text: request.text.clone(),
        });
        account.documents.insert(request.document, request.text);
        Box::pin(async { Ok::<_, RuntimeFailure>(Ok::<_, PutError>(PutResponse { stored: true })) })
    }

    fn read(
        &self,
        _context: InvocationContext,
        request: ReadRequest,
    ) -> NativeRequestFuture<DocumentStoreRead> {
        let result = {
            let mut account = self.0.inner.borrow_mut();
            account.calls.push(StoreCall::Read {
                document: request.document.clone(),
            });
            account.documents.get(&request.document).cloned()
        };
        Box::pin(async move {
            Ok(match result {
                Some(text) => Ok(ReadResponse { text }),
                None => Err(ReadError::NotFound),
            })
        })
    }
}
