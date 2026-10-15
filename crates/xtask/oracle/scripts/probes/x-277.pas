{ parity probe: x-277 }
unit P_x_277;

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
  v := UpperCase(True); AddMessage(v);
  AddMessage('after');
end;
end.
