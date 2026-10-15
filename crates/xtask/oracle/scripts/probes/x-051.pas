{ parity probe: x-051 }
unit P_x_051;

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
  s := String(1.5);
  AddMessage('after');
end;
end.
