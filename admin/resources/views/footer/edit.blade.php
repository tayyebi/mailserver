@extends('layouts.app')

@section('title', 'Email Footer')

@section('content')
<div class="card">
    <h2>Domain-Wide Email Footer</h2>
    <p style="color: #666; margin-bottom: 1.5rem;">
        This HTML footer is appended to all outgoing HTML emails before the closing <code>&lt;/body&gt;</code> tag.
        It is injected by the pixel milter alongside the tracking pixel. Leave empty to disable the footer.
    </p>

    <form action="{{ route('footer.update') }}" method="POST">
        @csrf
        @method('PUT')

        <div class="form-group">
            <label for="footer_html">Footer HTML</label>
            <textarea id="footer_html" name="footer_html" rows="16"
                      style="font-family: monospace; font-size: 13px; tab-size: 4;">{{ old('footer_html', $footerHtml) }}</textarea>
            @error('footer_html')
                <small style="color: red;">{{ $message }}</small>
            @enderror
            <small style="color: #666; display: block; margin-top: 0.5rem;">
                Use valid HTML. Inline styles are recommended for email client compatibility.
                The footer is inserted inside the email body, so avoid <code>&lt;html&gt;</code> or <code>&lt;body&gt;</code> tags.
            </small>
        </div>

        <div style="margin-top: 1rem;">
            <h3 style="margin-bottom: 0.5rem;">Preview</h3>
            <div id="footer-preview"
                 style="border: 1px solid #ddd; padding: 1rem; border-radius: 4px; background: #fff; min-height: 60px;">
            </div>
        </div>

        <div class="actions" style="margin-top: 1.5rem;">
            <button type="submit" class="btn btn-success">Save Footer</button>
            <button type="button" class="btn" onclick="resetToDefault()">Reset to Default</button>
        </div>
    </form>
</div>

<script>
    const textarea = document.getElementById('footer_html');
    const preview = document.getElementById('footer-preview');

    function updatePreview() {
        preview.innerHTML = textarea.value;
    }

    textarea.addEventListener('input', updatePreview);
    updatePreview();

    function resetToDefault() {
        if (!confirm('Reset the footer to the default template? Unsaved changes will be lost.')) return;
        textarea.value = `<div style="margin-top: 20px; padding: 15px; border-top: 1px solid #e0e0e0; font-size: 11px; color: #666; line-height: 1.6; font-family: Arial, sans-serif; background-color: #f9f9f9;">
  <p style="margin: 0 0 8px 0;">
    This email may contain tracking technology to measure engagement. You can disable tracking by disabling images in your email client.
  </p>
  <p style="margin: 0 0 8px 0;">
    For privacy concerns or to exercise your data protection rights, please contact the sender directly.
  </p>
  <p style="margin: 0;">
    This message is confidential and intended solely for the recipient. If you received this in error, please notify the sender and delete it.
  </p>
</div>`;
        updatePreview();
    }
</script>
@endsection
