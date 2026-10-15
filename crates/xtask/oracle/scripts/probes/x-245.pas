{ parity probe: x-245 }
unit P_x_245;

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
  v := ElementCount(sl);
  AddMessage('after');
end;
end.
