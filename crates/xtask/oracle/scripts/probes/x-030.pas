{ parity probe: x-030 }
unit P_x_030;

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
  case 'x' of 1: ; end;
  AddMessage('after');
end;
end.
