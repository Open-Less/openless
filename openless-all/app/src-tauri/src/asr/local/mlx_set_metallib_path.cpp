// MLX worker 启动时把 bundled metallib 路径交给 libmlx。
// mlx-c 没暴露 set_metallib_path，这里直接调 C++ API。

#include <string>

namespace mlx {
namespace core {
namespace metal {
void set_metallib_path(const std::string &path);
}
} // namespace core
} // namespace mlx

extern "C" int openless_mlx_set_metallib_path(const char *path) {
  if (path == nullptr || path[0] == '\0') {
    return 1;
  }
  mlx::core::metal::set_metallib_path(std::string(path));
  return 0;
}
