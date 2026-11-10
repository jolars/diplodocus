#pragma once
#include <array>
#include <vector>

/** @brief Examples of C++ API declarations. */
namespace demo {

/** @brief Available palette spaces. */
enum class Space {
  RGB, ///< Red, green, and blue.
  Lab  ///< Perceptual coordinates.
};

/**
 * @brief A reusable palette builder.
 *
 * See @ref Space for the available spaces.
 * - Configure the white point.
 * - Generate a palette.
 *
 * @code{.cpp}
 * demo::Builder builder;
 * builder.setWhitePoint(1);
 * @endcode
 */
class Builder {
public:
  /** @brief Construct a default builder. */
  Builder() = default;
  /** @brief Construct a builder with a size. @param size Palette size. */
  explicit Builder(int size);
  /**
   * @brief Set a standard white point.
   * @param point Standard white point index.
   * @return This builder for chaining.
   * @throws std::invalid_argument if the index is invalid.
   */
  Builder &setWhitePoint(int point);
  /**
   * @brief Set custom white point coordinates.
   * @param coordinates Three XYZ values.
   * @return This builder for chaining.
   */
  Builder &setWhitePoint(const std::array<double, 3> &coordinates);
  /** @brief Read a component. @param index Component index. @return The value. */
  double operator()(int index) const;
  /** @brief Read the selected size. @return Palette size. */
  int size() const;
  /** @brief Choose the output space. */
  Space space = Space::RGB;
protected:
  /** @brief The largest supported palette size. */
  static constexpr int limit = 256;
private:
  /** @brief Internal input mode. */
  enum class Mode { Default, Custom };
  int private_value = 0;
};

/** @brief A matrix of values. @tparam T Element type. */
template<typename T>
struct Matrix {
  /** @brief Stored elements. */
  std::vector<T> values;
  /** @brief Mutable access. @param index Element index. @return Element reference. */
  T &operator()(int index);
  /** @brief Constant access. @param index Element index. @return Element reference. */
  const T &operator()(int index) const;
};

/** @brief A named builder type. */
using Palette = Builder;

/** @brief Count values. @param values Input values. @return Their count. */
int count(const std::vector<double> &values);
/** @brief Count array values. @param values Input values. @return Their count. */
int count(const std::array<double, 3> &values);

namespace detail {
/** @brief An excluded implementation detail. */
struct Hidden {};
}
}
