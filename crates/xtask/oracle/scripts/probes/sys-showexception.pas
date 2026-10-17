{
  Parity fixture (issue #31): `ShowException` of `JvInterpreter_SysUtils`.

  Upstream is the RTL's `ShowException(ExceptObject, ExceptAddr)`: it puts
  up a modal message box (the exception's message, and for an object that
  is no `Exception` the `ExceptionErrorMessage` shape), titled `Application
  Error` with `OK`. The probe phase runs it unanswered: the oracle's run
  stops on the box, which the 120 s watch ends (`oracle-dialog`), and the
  port's headless VCL model stops with the question. The run case of
  runs.json answers the box on both sides (parity #41), so the line after
  the call is compared too.
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
