<script setup lang="ts">
import { useData } from 'vitepress'
import DefaultTheme from 'vitepress/theme'
import Motd from './Motd.vue'

const { Layout } = DefaultTheme
const { frontmatter } = useData()
</script>

<template>
  <Motd v-if="frontmatter.layout === 'motd'" />
  <Layout v-else />
</template>
