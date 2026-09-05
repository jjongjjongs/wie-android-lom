cmake_minimum_required(VERSION 3.22.1)
project(wie_audio_hook C)

add_library(wie_audio_hook SHARED wie_audio_hook.c)
target_link_libraries(wie_audio_hook android log dl)
target_link_options(wie_audio_hook PRIVATE
    "-Wl,-z,max-page-size=16384"
    "-Wl,-z,common-page-size=16384")
