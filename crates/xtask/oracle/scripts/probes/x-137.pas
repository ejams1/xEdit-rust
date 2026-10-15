{ parity probe: x-137 }
unit P_x_137;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  AddMessage(IntToStr(StrToInt(' 5')));
  AddMessage('after');
end;
end.
