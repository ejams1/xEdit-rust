{ parity probe: x-056 }
unit P_x_056;

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
  i := Integer(sl);
  AddMessage('after');
end;
end.
