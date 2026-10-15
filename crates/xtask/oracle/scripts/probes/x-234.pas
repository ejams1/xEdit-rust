{ parity probe: x-234 }
unit P_x_234;

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
  v := FileByIndex(0); AddMessage(IntToStr(v));
  AddMessage('after');
end;
end.
