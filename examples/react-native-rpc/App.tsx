import { useCallback, useMemo, useState } from "react";
import {
  ActivityIndicator,
  FlatList,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  SafeAreaView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { CometClient, CometRpcError, type Task } from "./src/comet-rpc";

const DEFAULT_API_URL = "http://127.0.0.1:8787";

function describeError(error: unknown) {
  if (error instanceof CometRpcError) {
    return `RPC ${error.status}: ${JSON.stringify(error.body)}`;
  }
  return error instanceof Error ? error.message : String(error);
}

function EmptyTasks() {
  return <Text style={styles.empty}>No tasks loaded.</Text>;
}

export default function App() {
  const [baseUrl, setBaseUrl] = useState(DEFAULT_API_URL);
  const [token, setToken] = useState("");
  const [title, setTitle] = useState("");
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState(
    "Set the API URL and bearer token, then load tasks.",
  );

  const client = useMemo(
    () =>
      new CometClient(baseUrl, () => {
        const trimmed = token.trim();
        return trimmed.length > 0 ? trimmed : undefined;
      }),
    [baseUrl, token],
  );

  const run = useCallback(async (action: () => Promise<void>) => {
    setLoading(true);
    try {
      await action();
    } catch (error) {
      setMessage(describeError(error));
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(
    () =>
      run(async () => {
        const nextTasks = await client.listTasks();
        setTasks(nextTasks);
        const suffix = nextTasks.length === 1 ? "" : "s";
        setMessage(`Loaded ${nextTasks.length} task${suffix}.`);
      }),
    [client, run],
  );

  const createTask = useCallback(
    () =>
      run(async () => {
        const trimmed = title.trim();
        if (!trimmed) {
          setMessage("Type a title before creating a task.");
          return;
        }
        const task = await client.createTask({ title: trimmed });
        setTitle("");
        setTasks((current) => [task, ...current]);
        setMessage(`Created task #${task.id}.`);
      }),
    [client, run, title],
  );

  const updateCompletedTask = useCallback((completed: Task) => {
    setTasks((current) =>
      current.map((item) => (item.id === completed.id ? completed : item)),
    );
  }, []);

  const completeTask = useCallback(
    (task: Task) =>
      run(async () => {
        const completed = await client.completeTask(task.id);
        updateCompletedTask(completed);
        setMessage(`Completed task #${completed.id}.`);
      }),
    [client, run, updateCompletedTask],
  );

  const keyExtractor = useCallback((item: Task) => String(item.id), []);

  const renderTask = useCallback(
    ({ item }: { item: Task }) => (
      <View style={styles.taskCard}>
        <View style={styles.taskText}>
          <Text style={styles.taskTitle}>{item.title}</Text>
          <Text style={styles.taskMeta}>
            #{item.id} - {item.created_at}
          </Text>
        </View>
        <Pressable
          disabled={loading || item.done}
          onPress={() => completeTask(item)}
          style={[styles.doneButton, item.done && styles.doneButtonDisabled]}
        >
          <Text style={styles.doneButtonText}>
            {item.done ? "Done" : "Complete"}
          </Text>
        </Pressable>
      </View>
    ),
    [completeTask, loading],
  );

  return (
    <SafeAreaView style={styles.safeArea}>
      <KeyboardAvoidingView
        behavior={Platform.OS === "ios" ? "padding" : undefined}
        style={styles.screen}
      >
        <View style={styles.header}>
          <Text style={styles.kicker}>Comet RPC</Text>
          <Text style={styles.title}>React Native client</Text>
        </View>

        <View style={styles.panel}>
          <Text style={styles.label}>API base URL</Text>
          <TextInput
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="url"
            onChangeText={setBaseUrl}
            placeholder="https://api.example.com"
            style={styles.input}
            value={baseUrl}
          />

          <Text style={styles.label}>Bearer token</Text>
          <TextInput
            autoCapitalize="none"
            autoCorrect={false}
            onChangeText={setToken}
            placeholder="Paste a session/access token"
            secureTextEntry
            style={styles.input}
            value={token}
          />

          <View style={styles.row}>
            <TextInput
              onChangeText={setTitle}
              placeholder="New task title"
              style={[styles.input, styles.taskInput]}
              value={title}
            />
            <Pressable disabled={loading} onPress={createTask} style={styles.primaryButton}>
              <Text style={styles.primaryButtonText}>Create</Text>
            </Pressable>
          </View>

          <Pressable disabled={loading} onPress={refresh} style={styles.secondaryButton}>
            <Text style={styles.secondaryButtonText}>Load tasks with listTasks()</Text>
          </Pressable>
        </View>

        <View style={styles.statusRow}>
          {loading ? <ActivityIndicator /> : null}
          <Text style={styles.statusText}>{message}</Text>
        </View>

        <FlatList
          contentContainerStyle={styles.listContent}
          data={tasks}
          keyExtractor={keyExtractor}
          ListEmptyComponent={EmptyTasks}
          renderItem={renderTask}
        />
      </KeyboardAvoidingView>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safeArea: {
    flex: 1,
    backgroundColor: "#f6f7f9",
  },
  screen: {
    flex: 1,
    padding: 20,
    gap: 16,
  },
  header: {
    gap: 4,
  },
  kicker: {
    color: "#52606d",
    fontSize: 13,
    fontWeight: "700",
    textTransform: "uppercase",
  },
  title: {
    color: "#101828",
    fontSize: 28,
    fontWeight: "800",
  },
  panel: {
    backgroundColor: "#ffffff",
    borderColor: "#e1e5ea",
    borderRadius: 8,
    borderWidth: 1,
    gap: 10,
    padding: 14,
  },
  label: {
    color: "#344054",
    fontSize: 13,
    fontWeight: "700",
  },
  input: {
    backgroundColor: "#ffffff",
    borderColor: "#cfd6df",
    borderRadius: 8,
    borderWidth: 1,
    color: "#101828",
    minHeight: 44,
    paddingHorizontal: 12,
  },
  row: {
    alignItems: "center",
    flexDirection: "row",
    gap: 10,
  },
  taskInput: {
    flex: 1,
  },
  primaryButton: {
    alignItems: "center",
    backgroundColor: "#176b5d",
    borderRadius: 8,
    minHeight: 44,
    justifyContent: "center",
    paddingHorizontal: 16,
  },
  primaryButtonText: {
    color: "#ffffff",
    fontWeight: "800",
  },
  secondaryButton: {
    alignItems: "center",
    borderColor: "#176b5d",
    borderRadius: 8,
    borderWidth: 1,
    minHeight: 44,
    justifyContent: "center",
  },
  secondaryButtonText: {
    color: "#176b5d",
    fontWeight: "800",
  },
  statusRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: 10,
    minHeight: 24,
  },
  statusText: {
    color: "#475467",
    flex: 1,
  },
  listContent: {
    gap: 10,
    paddingBottom: 32,
  },
  empty: {
    color: "#667085",
    paddingVertical: 20,
    textAlign: "center",
  },
  taskCard: {
    alignItems: "center",
    backgroundColor: "#ffffff",
    borderColor: "#e1e5ea",
    borderRadius: 8,
    borderWidth: 1,
    flexDirection: "row",
    gap: 12,
    padding: 14,
  },
  taskText: {
    flex: 1,
    gap: 4,
  },
  taskTitle: {
    color: "#101828",
    fontSize: 16,
    fontWeight: "700",
  },
  taskMeta: {
    color: "#667085",
    fontSize: 12,
  },
  doneButton: {
    backgroundColor: "#2f6fed",
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 10,
  },
  doneButtonDisabled: {
    backgroundColor: "#98a2b3",
  },
  doneButtonText: {
    color: "#ffffff",
    fontWeight: "800",
  },
});
