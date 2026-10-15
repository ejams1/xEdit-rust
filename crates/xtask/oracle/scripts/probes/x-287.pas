{ parity probe: x-287 }
unit P_x_287;

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
  sl.Add(sl);
  AddMessage('after');
end;
end.
