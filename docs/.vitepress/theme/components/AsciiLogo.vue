<script setup lang="ts">
// Pure-ASCII figlet "Standard" rendering of AWSIM.
const ascii =
'    _    __        ______ ___ __  __ \n' +
'   / \\   \\ \\      / / ___|_ _|  \\/  |\n' +
'  / _ \\   \\ \\ /\\ / /\\___ \\| || |\\/| |\n' +
' / ___ \\   \\ V  V /  ___) | || |  | |\n' +
'/_/   \\_\\   \\_/\\_/  |____/___|_|  |_|\n'
</script>

<template>
  <pre class="ascii">{{ ascii }}</pre>
</template>

<style scoped>
.ascii {
  margin: 0 0 4px;
  font-family: var(--awsim-mono);
  font-size: clamp(9px, 1.05vw, 13px);
  line-height: 1.05;
  font-weight: 500;
  color: var(--awsim-orange);
  text-shadow: 0 0 18px rgba(255, 153, 0, 0.45);
  white-space: pre;
  overflow: hidden;
  letter-spacing: 0;
}
</style>
