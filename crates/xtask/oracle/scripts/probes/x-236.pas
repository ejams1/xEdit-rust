{ parity probe: x-236 }
unit P_x_236;

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
  v := FileByIndex(0); i := v;
  AddMessage('after');
end;
end.
