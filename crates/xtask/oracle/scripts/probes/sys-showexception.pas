{
  Parity fixture (issue #31): `ShowException` of `JvInterpreter_SysUtils`.

  Upstream is the RTL's `ShowException(ExceptObject, ExceptAddr)`: it puts
  up a modal message box (the exception's message, and for an object that
  is no `Exception` the `ExceptionErrorMessage` shape). A headless run has
  no dialog; the port refuses the call through its GUI seam
  (`[EVclUnavailable] ShowException is not available without a GUI`), and
  the harness classes the oracle's run as `oracle-dialog`: the run stops on
  the box, which the 120 s watch ends. The probe pins that the call is
  reached (the line before it) and that nothing after it runs on a side
  without a window.
}
unit SysShowExceptionProbe;

function Initialize: integer;
var
  ex: Exception;
begin
  Result := 0;
  AddMessage('show | before');
  ex := Exception.Create('parity probe exception');
  ShowException(ex, nil);
  AddMessage('show | after');
end;

end.
