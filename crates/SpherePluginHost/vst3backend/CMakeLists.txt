cmake_minimum_required(VERSION 3.20)
project(SpherePluginHostVst3Backend LANGUAGES CXX)

set(SPHERE_VST3SDK_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../../../external/vst3sdk" CACHE PATH "Path to Steinberg VST3 SDK")

add_library(sphere_plugin_host_vst3 STATIC
  src/vst3_scanner.cpp
  ${SPHERE_VST3SDK_ROOT}/pluginterfaces/base/coreiids.cpp
  ${SPHERE_VST3SDK_ROOT}/pluginterfaces/base/funknown.cpp
  ${SPHERE_VST3SDK_ROOT}/pluginterfaces/base/ustring.cpp
  ${SPHERE_VST3SDK_ROOT}/public.sdk/source/common/commonstringconvert.cpp
  ${SPHERE_VST3SDK_ROOT}/public.sdk/source/vst/utility/stringconvert.cpp
  ${SPHERE_VST3SDK_ROOT}/public.sdk/source/vst/vstinitiids.cpp
  ${SPHERE_VST3SDK_ROOT}/public.sdk/source/vst/hosting/hostclasses.cpp
  ${SPHERE_VST3SDK_ROOT}/public.sdk/source/vst/hosting/pluginterfacesupport.cpp
  ${SPHERE_VST3SDK_ROOT}/public.sdk/source/vst/hosting/module.cpp
  ${SPHERE_VST3SDK_ROOT}/public.sdk/source/vst/hosting/module_win32.cpp
)

target_compile_features(sphere_plugin_host_vst3 PUBLIC cxx_std_17)
target_include_directories(sphere_plugin_host_vst3 PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/include
  ${SPHERE_VST3SDK_ROOT}
  ${SPHERE_VST3SDK_ROOT}/pluginterfaces
  ${SPHERE_VST3SDK_ROOT}/public.sdk/source
)
target_compile_definitions(sphere_plugin_host_vst3 PRIVATE
  SMTG_OS_WINDOWS=1
)
