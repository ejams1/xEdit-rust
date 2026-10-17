{
  Parity probe (issue #25): an external function with a Double parameter.
  CallDllIns refuses the argument type with ieDllInvalidArgument; the
  4.1.5q release faults (an access violation) on that path instead.
}
unit HostExtFunDouble;

function DoubleArg(d: Double): Integer; external 'kernel32.dll' name 'MulDiv';

function Initialize: Integer;
begin
  Result := 0;
  AddMessage('before');
  AddMessage(IntToStr(DoubleArg(1.5)));
  AddMessage('after');
end;

end.
