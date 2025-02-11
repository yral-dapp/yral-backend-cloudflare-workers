export default {
    async tail(events) {
        console.log(events);

        for (const event of events) {
            if (event.logs && event.logs.length > 0) {
                for (const log of event.logs) {

                    // console.log(log);

                    // Add scriptName to the log object and concatenate messages
                    const enrichedLog = {
                        ...log,
                        app: event.scriptName,
                        message: Array.isArray(log.message) ? log.message.join(' ; ') : log.message
                    };

                    // console.log(JSON.stringify(enrichedLog));

                    try {
                        const response = await fetch('https://vector-dev-tmp.fly.dev/', {
                            method: 'POST',
                            headers: {
                                'Content-Type': 'application/json',
                            },
                            body: JSON.stringify(enrichedLog)
                        });
                        
                        if (!response.ok) {
                            console.error(`Failed to send log: ${response.status} ${response.statusText}`);
                        }
                    } catch (error) {
                        console.error('Error sending log:', error);
                    }
                }
            }
        }
    }
}