#include <napi.h>

typedef struct TSLanguage TSLanguage;
extern "C" TSLanguage *tree_sitter_cairn();

namespace {

// Type tag used to mark the External<TSLanguage> value so consumers (e.g.
// node-tree-sitter) can verify at runtime that the pointer they received is
// actually a TSLanguage* before dereferencing it. The two 64-bit words are
// arbitrary but must stay fixed for this binding's lifetime.
napi_type_tag LANGUAGE_TYPE_TAG = {0x1C4CD379FD3C4C3C, 0x8E8394F4C7B2B4D8};

Napi::Object Init(Napi::Env env, Napi::Object exports) {
  exports["name"] = Napi::String::New(env, "cairn");
  auto language = Napi::External<TSLanguage>::New(env, tree_sitter_cairn());
  language.TypeTag(&LANGUAGE_TYPE_TAG);
  exports["language"] = language;
  return exports;
}

} // namespace

NODE_API_MODULE(tree_sitter_cairn_binding, Init)
