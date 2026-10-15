{ parity probe: x-057 }
unit P_x_057;

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
  v := TStringList(5);
  AddMessage('after');
end;
end.
