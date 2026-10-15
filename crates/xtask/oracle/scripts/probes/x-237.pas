{ parity probe: x-237 }
unit P_x_237;

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
  v := FileByIndex(0); b := v;
  AddMessage('after');
end;
end.
